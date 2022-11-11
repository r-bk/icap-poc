use crate::{
    server::ReqCtx,
    service::{IcapService, ServiceResult},
};
use std::{boxed::Box, future::Future, sync::Arc};

pub struct ServiceFn<GCTX, OP, OPF, RQ, RQF, RS, RSF>
where
    GCTX: Send + Sync,
    OPF: Future<Output = ServiceResult<GCTX>> + Send,
    OP: Clone + FnMut(Box<ReqCtx<GCTX>>) -> OPF,
    RQF: Future<Output = ServiceResult<GCTX>> + Send,
    RQ: Clone + FnMut(Box<ReqCtx<GCTX>>) -> RQF,
    RSF: Future<Output = ServiceResult<GCTX>> + Send,
    RS: Clone + FnMut(Box<ReqCtx<GCTX>>) -> RSF,
{
    global_ctx: Option<Arc<GCTX>>,
    handle_options: OP,
    handle_reqmod: RQ,
    handle_respmod: RS,
}

impl<GCTX, OP, OPF, RQ, RQF, RS, RSF> IcapService<GCTX>
    for ServiceFn<GCTX, OP, OPF, RQ, RQF, RS, RSF>
where
    GCTX: Send + Sync,
    OPF: Future<Output = ServiceResult<GCTX>> + Send,
    OP: Clone + FnMut(Box<ReqCtx<GCTX>>) -> OPF,
    RQF: Future<Output = ServiceResult<GCTX>> + Send,
    RQ: Clone + FnMut(Box<ReqCtx<GCTX>>) -> RQF,
    RSF: Future<Output = ServiceResult<GCTX>> + Send,
    RS: Clone + FnMut(Box<ReqCtx<GCTX>>) -> RSF,
{
    type OPF = OPF;
    type RQF = RQF;
    type RSF = RSF;

    #[inline]
    fn take_global_ctx(&mut self) -> Option<Arc<GCTX>> {
        self.global_ctx.take()
    }

    #[inline]
    fn handle_options(&mut self, ctx: Box<ReqCtx<GCTX>>) -> Self::OPF {
        (self.handle_options)(ctx)
    }

    #[inline]
    fn handle_reqmod(&mut self, ctx: Box<ReqCtx<GCTX>>) -> Self::RQF {
        (self.handle_reqmod)(ctx)
    }

    #[inline]
    fn handle_respmod(&mut self, ctx: Box<ReqCtx<GCTX>>) -> Self::RSF {
        (self.handle_respmod)(ctx)
    }
}

impl<GCTX, OP, OPF, RQ, RQF, RS, RSF> Clone for ServiceFn<GCTX, OP, OPF, RQ, RQF, RS, RSF>
where
    GCTX: Send + Sync,
    OPF: Future<Output = ServiceResult<GCTX>> + Send,
    OP: Clone + FnMut(Box<ReqCtx<GCTX>>) -> OPF,
    RQF: Future<Output = ServiceResult<GCTX>> + Send,
    RQ: Clone + FnMut(Box<ReqCtx<GCTX>>) -> RQF,
    RSF: Future<Output = ServiceResult<GCTX>> + Send,
    RS: Clone + FnMut(Box<ReqCtx<GCTX>>) -> RSF,
{
    #[inline]
    fn clone(&self) -> Self {
        Self {
            global_ctx: self.global_ctx.clone(),
            handle_options: self.handle_options.clone(),
            handle_reqmod: self.handle_reqmod.clone(),
            handle_respmod: self.handle_respmod.clone(),
        }
    }
}

#[inline]
pub fn service_fn<GCTX, OP, OPF, RQ, RQF, RS, RSF>(
    global_ctx: Option<Arc<GCTX>>,
    handle_options: OP,
    handle_reqmod: RQ,
    handle_respmod: RS,
) -> ServiceFn<GCTX, OP, OPF, RQ, RQF, RS, RSF>
where
    GCTX: Send + Sync,
    OPF: Future<Output = ServiceResult<GCTX>> + Send,
    OP: Clone + FnMut(Box<ReqCtx<GCTX>>) -> OPF,
    RQF: Future<Output = ServiceResult<GCTX>> + Send,
    RQ: Clone + FnMut(Box<ReqCtx<GCTX>>) -> RQF,
    RSF: Future<Output = ServiceResult<GCTX>> + Send,
    RS: Clone + FnMut(Box<ReqCtx<GCTX>>) -> RSF,
{
    ServiceFn {
        global_ctx,
        handle_options,
        handle_reqmod,
        handle_respmod,
    }
}
