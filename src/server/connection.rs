use crate::{
    common::Id,
    decoder::{decode_chunk_header, DecodingStatus},
    errors::ConnectionError,
    server::{AdaptationDecision::*, ReqCtx, ReqCtxBox, RBUF_CAP},
    service::IcapService,
    Method, Version,
};
use bytes::{BufMut, BytesMut};
use http::StatusCode;
use std::{
    fmt::Write,
    io::{self, ErrorKind},
    slice,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tracing::{debug, error, instrument, trace, warn};

#[derive(Debug)]
enum ProcessingDecision {
    Continue(ReqCtxBox),
    Shutdown,
}

type ConnectionResult = Result<ProcessingDecision, ConnectionError>;

#[derive(Debug)]
pub struct Connection<S> {
    pub id: Id,
    sock: TcpStream,
    wbuf: BytesMut,
    svc: S,
}

impl<S> Connection<S>
where
    S: IcapService,
    <S as IcapService>::OPF: Send,
    <S as IcapService>::RQF: Send,
    <S as IcapService>::RSF: Send,
{
    pub fn new(id: Id, sock: TcpStream, svc: S) -> Self {
        Connection {
            id,
            sock,
            wbuf: BytesMut::with_capacity(512),
            svc,
        }
    }

    #[instrument(name = "connection", skip(self), fields(id = %self.id))]
    pub async fn process(&mut self) {
        if self.sock.set_nodelay(true).is_err() {
            error!("failed to set TCP_NODELAY");
        }

        let mut ctx = ReqCtx::new_box();
        loop {
            ctx.msgs_cnt += 1;
            ctx = match self.process_message(ctx).await {
                Ok(ProcessingDecision::Continue(c)) => c,
                Ok(ProcessingDecision::Shutdown) => break,
                Err(e) => {
                    error!(err=%e, "process_message exited with error");
                    break;
                }
            };
            ctx.clear();
        }
        trace!("shutting down connection");
        if let Err(e) = self.sock.shutdown().await {
            warn!(err=%e, "socket.shutdown failed");
        }
    }

    #[instrument(name = "message", skip(self, ctx), fields(n = ctx.msgs_cnt), err)]
    async fn process_message(&mut self, mut ctx: ReqCtxBox) -> ConnectionResult {
        ctx = match self.init_ctx(ctx).await {
            Ok(ctx) => ctx,
            Err(ConnectionError::Decoder(e)) => {
                error!("failed to decode message: {}", e);
                return self.send_status(StatusCode::BAD_REQUEST).await;
            }
            _ => return Ok(ProcessingDecision::Shutdown),
        };

        ctx = match self.recv_missing_header_bytes(ctx).await? {
            ProcessingDecision::Continue(ctx) => ctx,
            ProcessingDecision::Shutdown => return Ok(ProcessingDecision::Shutdown),
        };

        if ctx.icap_req.method.is_any_req() && !ctx.null_body {
            ctx = match self.recv_preview_zero_chunk(ctx).await? {
                ProcessingDecision::Continue(ctx) => ctx,
                ProcessingDecision::Shutdown => return Ok(ProcessingDecision::Shutdown),
            };
        }

        // now that all data is in ctx.rbuf, it is not expected to be reallocated
        // so we can parse the headers and build header indices
        if let Err(e) = ctx.parse_entities() {
            error!(err = %e, "parse_entities failed");
            return self.send_status(StatusCode::BAD_REQUEST).await;
        }

        match ctx.icap_req.method {
            Method::Options => self.process_options(ctx).await,
            Method::ReqMod => self.process_reqmod(ctx).await,
            Method::RespMod => self.process_respmod(ctx).await,
        }
    }

    async fn process_options(&mut self, ctx: ReqCtxBox) -> ConnectionResult {
        let mut ctx = match self.svc.handle_options(ctx).await {
            Ok(ctx) => ctx,
            Err(e) => {
                error!(err = %e, "handle_options failed");
                return self.send_status(StatusCode::INTERNAL_SERVER_ERROR).await;
            }
        };
        ctx.ensure_options_headers();
        self.wbuf.clear();
        write!(
            self.wbuf,
            "{} {}\r\n",
            Version::Icap10.as_str(),
            ctx.out_icap_status.unwrap_or(StatusCode::OK),
        )?;
        write_headers_map(&mut self.wbuf, &ctx.out_icap_headers);
        self.wbuf.extend_from_slice(b"\r\n");
        self.sock.write_all(&self.wbuf).await?;
        Ok(ProcessingDecision::Continue(ctx))
    }

    async fn process_reqmod(&mut self, ctx: ReqCtxBox) -> ConnectionResult {
        let ctx = match self.svc.handle_reqmod(ctx).await {
            Ok(ctx) => ctx,
            Err(e) => {
                error!(err = %e, "handle_reqmod failed");
                return self.send_status(StatusCode::INTERNAL_SERVER_ERROR).await;
            }
        };
        self.process_decision(ctx).await
    }

    async fn process_respmod(&mut self, ctx: ReqCtxBox) -> ConnectionResult {
        let ctx = match self.svc.handle_respmod(ctx).await {
            Ok(ctx) => ctx,
            Err(e) => {
                error!(err = %e, "handle_respmod failed");
                return self.send_status(StatusCode::INTERNAL_SERVER_ERROR).await;
            }
        };
        self.process_decision(ctx).await
    }

    async fn process_decision(&mut self, ctx: ReqCtxBox) -> ConnectionResult {
        let decision = match ctx.decision {
            Some(d) => d,
            None => {
                error!("adaptation decision wasn't set");
                return self.send_status(StatusCode::INTERNAL_SERVER_ERROR).await;
            }
        };
        match decision {
            NoAdaptation => self.send_204(ctx).await,
            AppendHeaders => self.append_headers(ctx).await,
            CustomResponse => self.custom_response(ctx).await,
        }
    }

    async fn send_204(&mut self, mut ctx: ReqCtxBox) -> ConnectionResult {
        ctx.ensure_204_headers();
        self.wbuf.clear();
        write!(
            self.wbuf,
            "{} {}\r\n",
            Version::Icap10.as_str(),
            StatusCode::NO_CONTENT
        )?;
        write_headers_map(&mut self.wbuf, &ctx.out_icap_headers);
        self.wbuf.extend_from_slice(b"\r\n");
        self.sock.write_all(&self.wbuf).await?;
        Ok(ProcessingDecision::Continue(ctx))
    }

    async fn append_headers(&mut self, mut ctx: ReqCtxBox) -> ConnectionResult {
        ctx.ensure_response_headers();
        self.wbuf.clear();
        ctx.http_buf.clear();

        let ee = match ctx.icap_req.method {
            Method::ReqMod => {
                write!(
                    ctx.http_buf,
                    "{} {} {:?}\r\n",
                    ctx.http_req.method, ctx.http_req.uri, ctx.http_req.version
                )?;
                ctx.http_req.headers.encode(&ctx.rbuf, &mut ctx.http_buf);
                "req"
            }
            Method::RespMod => {
                write!(
                    ctx.http_buf,
                    "{:?} {}\r\n",
                    ctx.http_res.version, ctx.http_res.status
                )?;
                ctx.http_res.headers.encode(&ctx.rbuf, &mut ctx.http_buf);
                "res"
            }
            _ => panic!("should not get here"),
        };

        write_headers_map(&mut ctx.http_buf, &ctx.out_http_headers);
        ctx.http_buf.extend_from_slice(b"\r\n");

        let body_off = ctx.http_buf.len();

        if !ctx.null_body {
            ctx.http_buf
                .extend_from_slice(b"0; use-original-body=0\r\n\r\n");
        }

        let (enc, status) = if !ctx.null_body {
            (
                cds::aformat!(128, "{}-hdr=0, {}-body={}", ee, ee, body_off)?,
                StatusCode::PARTIAL_CONTENT,
            )
        } else {
            (
                cds::aformat!(128, "{}-hdr=0, null-body={}", ee, body_off)?,
                StatusCode::OK,
            )
        };

        write!(self.wbuf, "{} {}\r\n", Version::Icap10, status)?;
        write_headers_map(&mut self.wbuf, &ctx.out_icap_headers);
        self.wbuf.extend_from_slice(b"Encapsulated: ");
        self.wbuf.extend_from_slice(enc.as_bytes());
        self.wbuf.extend_from_slice(b"\r\n\r\n");

        self.sock.write_all(&self.wbuf).await?;
        self.sock.write_all(&ctx.http_buf).await?;

        Ok(ProcessingDecision::Continue(ctx))
    }

    async fn custom_response(&mut self, mut ctx: ReqCtxBox) -> ConnectionResult {
        ctx.ensure_response_headers();
        self.wbuf.clear();
        ctx.http_buf.clear();

        let http_status = match ctx.out_http_status {
            Some(s) => s,
            None => {
                error!("custom response status not set");
                return self.send_status(StatusCode::INTERNAL_SERVER_ERROR).await;
            }
        };

        let http_version = match ctx.out_http_ver {
            Some(v) => v,
            None => {
                if ctx.http_req.parsed_len != 0 {
                    ctx.http_req.version
                } else {
                    error!("cannot determine http-req version");
                    http::Version::HTTP_11
                }
            }
        };

        write!(ctx.http_buf, "{:?} {}\r\n", http_version, http_status)?;
        write_headers_map(&mut ctx.http_buf, &ctx.out_http_headers);
        ctx.http_buf.extend_from_slice(b"\r\n");

        write!(self.wbuf, "{} {}\r\n", Version::Icap10, StatusCode::OK)?;
        write_headers_map(&mut self.wbuf, &ctx.out_icap_headers);

        let enc = cds::aformat!(128, "res-hdr=0, null-body={}\r\n", ctx.http_buf.len())?;
        self.wbuf.extend_from_slice(b"Encapsulated: ");
        self.wbuf.extend_from_slice(enc.as_bytes());
        self.wbuf.extend_from_slice(b"\r\n");

        self.sock.write_all(&self.wbuf).await?;
        self.sock.write_all(&ctx.http_buf).await?;

        Ok(ProcessingDecision::Continue(ctx))
    }

    #[instrument(skip(self, ctx))]
    async fn init_ctx(&mut self, mut ctx: ReqCtxBox) -> Result<ReqCtxBox, ConnectionError> {
        loop {
            match ctx.init()? {
                DecodingStatus::Complete => {
                    return Ok(ctx);
                }
                DecodingStatus::Partial => {
                    let n = self.recv(&mut ctx.rbuf).await?;
                    if n == 0 {
                        debug!("incoming connection closed");
                        return Err(io::Error::from(ErrorKind::ConnectionReset).into());
                    }
                }
            }
        }
    }

    #[instrument(skip(self))]
    async fn send_status(&mut self, status: StatusCode) -> ConnectionResult {
        debug_assert!(status.is_client_error() || status.is_server_error());
        self.wbuf.clear();
        write!(self.wbuf, "{} {}\r\n", Version::Icap10.as_str(), status)?;
        write!(self.wbuf, "ISTag: \"r-bk-icap\"")?;
        write!(self.wbuf, "Connection: close\r\n")?;
        write!(self.wbuf, "Encapsulated: null-body=0\r\n")?;
        write!(self.wbuf, "\r\n")?;
        self.sock.write_all(&self.wbuf).await?;
        Ok(ProcessingDecision::Shutdown)
    }

    #[instrument(skip(self, ctx), err)]
    async fn recv_missing_header_bytes(&mut self, mut ctx: ReqCtxBox) -> ConnectionResult {
        let mut missing_bytes = ctx.header_missing_bytes;
        loop {
            if missing_bytes == 0 {
                trace!("no missing bytes left");
                break;
            }
            let chunk = ctx.rbuf.chunk_mut();
            let slc = unsafe { slice::from_raw_parts_mut(chunk.as_mut_ptr(), chunk.len()) };
            debug_assert!(slc.len() >= missing_bytes);
            trace!("reading up to {} bytes", slc.len());
            let n = self.sock.read(slc).await?;
            unsafe { ctx.rbuf.advance_mut(n) };
            trace!("received {} bytes", n);
            missing_bytes -= missing_bytes.min(n);
            if n == 0 && missing_bytes > 0 {
                debug!("incoming connection closed while there are missing header bytes");
                return Ok(ProcessingDecision::Shutdown);
            }
        }
        Ok(ProcessingDecision::Continue(ctx))
    }

    #[instrument(skip(self, ctx), err)]
    async fn recv_preview_zero_chunk(&mut self, mut ctx: ReqCtxBox) -> ConnectionResult {
        let body_buf_offset = ctx.icap_req.parsed_len + ctx.body_offset;
        debug_assert!(ctx.rbuf.len() >= body_buf_offset);
        trace!(
            body_buf_offset = body_buf_offset,
            "calculated body buffer offset"
        );

        loop {
            let body_slc = &ctx.rbuf[body_buf_offset..];

            let chunk_hdr = match decode_chunk_header(body_slc) {
                Ok(None) => {
                    trace!("partial chunk header");
                    let n = self.recv(&mut ctx.rbuf).await?;
                    if n == 0 {
                        debug!("incoming connection closed at preview chunk");
                        return Ok(ProcessingDecision::Shutdown);
                    }
                    continue;
                }
                Ok(Some(hdr)) => hdr,
                Err(e) => {
                    error!(err = %e, "failed to decode chunk header");
                    return self.send_status(StatusCode::BAD_REQUEST).await;
                }
            };

            if chunk_hdr.chunk_len != 0 {
                error!(chunk_hdr = ?chunk_hdr, "unexpected chunk length");
                return self.send_status(StatusCode::BAD_REQUEST).await;
            }

            if body_slc.len() < chunk_hdr.line_len + 2 {
                trace!("missing final CRLF");
                let n = self.recv(&mut ctx.rbuf).await?;
                if n == 0 {
                    debug!("incoming connection closed at preview final CRLF");
                    return Ok(ProcessingDecision::Shutdown);
                }
                continue;
            }

            let crlf_slc = &body_slc[chunk_hdr.line_len..(chunk_hdr.line_len + 2)];
            if crlf_slc != b"\r\n" {
                error!(crlf_cls = ?crlf_slc, "failed to parse chunk header final CRLF");
                return self.send_status(StatusCode::BAD_REQUEST).await;
            }

            break Ok(ProcessingDecision::Continue(ctx));
        }
    }

    async fn recv(&mut self, rbuf: &mut BytesMut) -> io::Result<usize> {
        if rbuf.capacity() <= 1024 {
            rbuf.reserve(RBUF_CAP);
        }
        let chunk = rbuf.chunk_mut();
        let slc = unsafe { slice::from_raw_parts_mut(chunk.as_mut_ptr(), chunk.len()) };
        trace!("reading up to {} bytes", slc.len());
        let n = self.sock.read(slc).await?;
        unsafe { rbuf.advance_mut(n) };
        trace!("received {} bytes", n);
        Ok(n)
    }
}

fn write_headers_map(buf: &mut BytesMut, headers: &http::HeaderMap) {
    for (k, v) in headers.iter() {
        buf.extend_from_slice(k.as_str().as_bytes());
        buf.extend_from_slice(b": ");
        buf.extend_from_slice(v.as_bytes());
        buf.extend_from_slice(b"\r\n");
    }
}
