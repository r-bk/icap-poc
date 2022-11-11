use crate::server::ReqCtx;
use std::{boxed::Box, future::Future, sync::Arc};

mod error_code;
pub use error_code::*;

pub type ServiceResult<GCTX> = Result<Box<ReqCtx<GCTX>>, ErrorCode>;

pub trait IcapService<GCTX>: Clone
where
    GCTX: Send + Sync,
{
    type OPF: Future<Output = ServiceResult<GCTX>>;
    type RQF: Future<Output = ServiceResult<GCTX>>;
    type RSF: Future<Output = ServiceResult<GCTX>>;

    fn take_global_ctx(&mut self) -> Option<Arc<GCTX>>;

    fn handle_options(&mut self, ctx: Box<ReqCtx<GCTX>>) -> Self::OPF;

    fn handle_reqmod(&mut self, ctx: Box<ReqCtx<GCTX>>) -> Self::RQF;

    fn handle_respmod(&mut self, ctx: Box<ReqCtx<GCTX>>) -> Self::RSF;
}
