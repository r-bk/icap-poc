use bytes::BytesMut;

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Default)]
pub(crate) struct HeaderIndices {
    pub(crate) name: (usize, usize),
    pub(crate) value: (usize, usize),
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Default)]
pub(crate) struct HeaderIndicesList {
    pub(crate) vec: Vec<HeaderIndices>,
    pub(crate) base_ptr: usize,
}

impl HeaderIndicesList {
    #[inline]
    pub fn clear(&mut self) {
        self.vec.clear();
        self.base_ptr = 0;
    }

    #[inline]
    pub fn encode(&self, data_buf: &BytesMut, wbuf: &mut BytesMut) {
        for hi in self.vec.iter() {
            let name = &data_buf[hi.name.0..hi.name.1];
            let value = &data_buf[hi.value.0..hi.value.1];
            wbuf.extend_from_slice(name);
            wbuf.extend_from_slice(b": ");
            wbuf.extend_from_slice(value);
            wbuf.extend_from_slice(b"\r\n");
        }
    }
}
