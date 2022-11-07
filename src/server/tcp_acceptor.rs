use crate::{common::CONN_ID, server::Connection, service::IcapService};
use std::{io::Result, net::SocketAddr};
use tokio::{
    net::{TcpListener, TcpSocket},
    task,
};
use tracing::{debug, instrument, trace};

#[derive(Debug)]
pub struct TcpAcceptor<S>
where
    S: IcapService + Send + 'static,
    <S as IcapService>::OPF: Send,
    <S as IcapService>::RQF: Send,
    <S as IcapService>::RSF: Send,
{
    sock: TcpListener,
    local_addr: SocketAddr,
    svc: S,
}

impl<S> TcpAcceptor<S>
where
    S: IcapService + Send + 'static,
    <S as IcapService>::OPF: Send,
    <S as IcapService>::RQF: Send,
    <S as IcapService>::RSF: Send,
{
    pub async fn bind(svc: S, addr: SocketAddr, backlog: u32) -> Result<Self> {
        let sock = if addr.is_ipv4() {
            TcpSocket::new_v4()?
        } else {
            TcpSocket::new_v6()?
        };

        sock.set_reuseaddr(true)?;
        sock.set_reuseport(true)?;
        sock.bind(addr)?;

        let local_addr = sock.local_addr()?;
        Ok(Self {
            sock: sock.listen(backlog)?,
            local_addr,
            svc,
        })
    }

    #[instrument(name = "tcp_acceptor", skip(self), fields(addr=%self.local_addr))]
    pub async fn run(&self) -> Result<()> {
        trace!("start...");
        loop {
            let (sock, addr) = self.sock.accept().await?;
            let conn_id = CONN_ID.next();
            let svc = self.svc.clone();
            debug!(addr = %addr, id=%conn_id, "accepted new connection");

            task::spawn(async move {
                let mut conn = Connection::new(conn_id, sock, svc);
                conn.process().await;
                trace!(id=%conn.id, "connection terminated");
            });
        }
    }
}
