use crate::{common::CONN_ID, server::Connection, service::IcapService};
use std::{io::Result, net::SocketAddr};
use tokio::{
    net::{TcpListener, ToSocketAddrs},
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
    pub async fn bind<A: ToSocketAddrs>(svc: S, addr: A) -> Result<Self> {
        let sock = TcpListener::bind(addr).await?;
        let local_addr = sock.local_addr()?;
        Ok(Self {
            sock,
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
