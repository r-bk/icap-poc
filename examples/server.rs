use icap_poc::{
    server::{AdaptationDecision::*, ReqCtx, ServerCfg, TcpAcceptor},
    service::ServiceResult,
    service_fn,
};

use http::StatusCode;
use std::{boxed::Box, io::Result};
use tracing::instrument;

#[instrument(err)]
async fn handle_options(mut ctx: Box<ReqCtx>) -> ServiceResult {
    ctx.set_icap_status(StatusCode::OK);
    ctx.append_icap_res_header("Server", "r-bk/icap");
    ctx.append_icap_res_header("Service", "r-bk/icap server example");
    ctx.append_icap_res_header("ISTag", "\"V-001\"");
    ctx.append_icap_res_header("Allow", "204, 206");
    ctx.append_icap_res_header("Methods", "REQMOD, RESPMOD");
    ctx.append_icap_res_header("Preview", "0");
    ctx.append_icap_res_header("Transfer-Preview", "*");
    ctx.append_icap_res_header("Connection", "keep-alive");
    Ok(ctx)
}

#[instrument(err)]
async fn handle_reqmod(mut ctx: Box<ReqCtx>) -> ServiceResult {
    ctx.set_decision(AppendHeaders);
    ctx.append_http_header("X-Appended-1", "Val-1");
    ctx.append_http_header("X-Appended-2", "Val-2");
    Ok(ctx)
}

#[instrument(err)]
async fn handle_respmod(mut ctx: Box<ReqCtx>) -> ServiceResult {
    ctx.set_decision(CustomResponse);
    ctx.set_http_status(StatusCode::TEMPORARY_REDIRECT);
    ctx.append_http_header("Location", "https://cnn.com");
    Ok(ctx)
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cfg = ServerCfg::builder().build();

    let svc = service_fn(cfg, handle_options, handle_reqmod, handle_respmod);

    let l = TcpAcceptor::bind(svc, "127.0.0.1:1344").await.unwrap();

    l.run().await
}
