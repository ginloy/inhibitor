use tokio_util::{
    bytes::{Buf, BytesMut},
    codec::{Decoder, Encoder},
};

use crate::Signal;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignalCodec {}
impl Decoder for SignalCodec {
    type Item = Signal;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.is_empty() {
            return Ok(None);
        }
        let size: u8 = src[0];
        if src[1..].len() < size as usize {
            return Ok(None);
        }
        let data = &src[1..(1 + size as usize)];
        match rkyv::from_bytes::<Signal>(data) {
            Ok(s) => {
                src.advance(1 + size as usize);
                Ok(Some(s))
            }
            Err(_) => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Failed to deserialize",
            )),
        }
    }
}

impl Encoder<Signal> for SignalCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: Signal, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let bytes = rkyv::to_bytes::<_, 64>(&item)
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "Failed to serialize"))?;
        let len_slice = u8::to_le_bytes(bytes.len() as u8);
        dst.reserve(1 + bytes.len());
        dst.extend_from_slice(&len_slice);
        dst.extend_from_slice(&bytes);
        Ok(())
    }
}
