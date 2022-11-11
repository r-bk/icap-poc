use crate::{
    server::ReqCtx,
    service::{IcapService, ServiceResult},
};
use std::{boxed::Box, future::Future};

pub struct ServiceFn<OP, OPF, RQ, RQF, RS, RSF>
where
    OPF: Future<Output = ServiceResult> + Send,
    OP: Clone + FnMut(Box<ReqCtx>) -> OPF,
    RQF: Future<Output = ServiceResult> + Send,
    RQ: Clone + FnMut(Box<ReqCtx>) -> RQF,
    RSF: Future<Output = ServiceResult> + Send,
    RS: Clone + FnMut(Box<ReqCtx>) -> RSF,
{
    handle_options: OP,
    handle_reqmod: RQ,
    handle_respmod: RS,
}

impl<OP, OPF, RQ, RQF, RS, RSF> IcapService for ServiceFn<OP, OPF, RQ, RQF, RS, RSF>
where
    OPF: Future<Output = ServiceResult> + Send,
    OP: Clone + FnMut(Box<ReqCtx>) -> OPF,
    RQF: Future<Output = ServiceResult> + Send,
    RQ: Clone + FnMut(Box<ReqCtx>) -> RQF,
    RSF: Future<Output = ServiceResult> + Send,
    RS: Clone + FnMut(Box<ReqCtx>) -> RSF,
{
    type OPF = OPF;
    type RQF = RQF;
    type RSF = RSF;

    #[inline]
    fn handle_options(&mut self, ctx: Box<ReqCtx>) -> Self::OPF {
        (self.handle_options)(ctx)
    }

    #[inline]
    fn handle_reqmod(&mut self, ctx: Box<ReqCtx>) -> Self::RQF {
        (self.handle_reqmod)(ctx)
    }

    #[inline]
    fn handle_respmod(&mut self, ctx: Box<ReqCtx>) -> Self::RSF {
        (self.handle_respmod)(ctx)
    }
}

impl<OP, OPF, RQ, RQF, RS, RSF> Clone for ServiceFn<OP, OPF, RQ, RQF, RS, RSF>
where
    OPF: Future<Output = ServiceResult> + Send,
    OP: Clone + FnMut(Box<ReqCtx>) -> OPF,
    RQF: Future<Output = ServiceResult> + Send,
    RQ: Clone + FnMut(Box<ReqCtx>) -> RQF,
    RSF: Future<Output = ServiceResult> + Send,
    RS: Clone + FnMut(Box<ReqCtx>) -> RSF,
{
    #[inline]
    fn clone(&self) -> Self {
        Self {
            handle_options: self.handle_options.clone(),
            handle_reqmod: self.handle_reqmod.clone(),
            handle_respmod: self.handle_respmod.clone(),
        }
    }
}

#[inline]
pub fn service_fn<OP, OPF, RQ, RQF, RS, RSF>(
    handle_options: OP,
    handle_reqmod: RQ,
    handle_respmod: RS,
) -> ServiceFn<OP, OPF, RQ, RQF, RS, RSF>
where
    OPF: Future<Output = ServiceResult> + Send,
    OP: Clone + FnMut(Box<ReqCtx>) -> OPF,
    RQF: Future<Output = ServiceResult> + Send,
    RQ: Clone + FnMut(Box<ReqCtx>) -> RQF,
    RSF: Future<Output = ServiceResult> + Send,
    RS: Clone + FnMut(Box<ReqCtx>) -> RSF,
{
    ServiceFn {
        handle_options,
        handle_reqmod,
        handle_respmod,
    }
}
