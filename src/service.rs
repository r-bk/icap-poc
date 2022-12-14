use crate::server::{ReqCtx, ServerCfg};
use std::{boxed::Box, future::Future, sync::Arc};

mod error_code;
pub use error_code::*;

pub type ServiceResult = Result<Box<ReqCtx>, ErrorCode>;

pub trait IcapService: Clone {
    type OPF: Future<Output = ServiceResult>;
    type RQF: Future<Output = ServiceResult>;
    type RSF: Future<Output = ServiceResult>;

    fn server_cfg(&self) -> Arc<ServerCfg>;

    fn handle_options(&mut self, ctx: Box<ReqCtx>) -> Self::OPF;

    fn handle_reqmod(&mut self, ctx: Box<ReqCtx>) -> Self::RQF;

    fn handle_respmod(&mut self, ctx: Box<ReqCtx>) -> Self::RSF;
}
