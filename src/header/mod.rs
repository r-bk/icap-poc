mod header_indices;
mod header_iterator;
mod header_name;
mod header_value;

pub(crate) use header_indices::*;
pub use header_iterator::*;
pub use header_name::*;
pub use header_value::*;

#[derive(Debug, Clone, Eq, PartialEq)]
#[non_exhaustive]
pub struct Header<'b> {
    pub name: HeaderName<'b>,
    pub value: HeaderValue<'b>,
}
