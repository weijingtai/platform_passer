use crate::Frame;
use anyhow::Result;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, frame: &Frame) -> Result<()> {
    let bytes = bincode::serialize(frame)?;
    let len = bytes.len() as u32;
    writer.write_u32_le(len).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}

pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Option<Frame>> {
    let len = match reader.read_u32_le().await {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    };

    let mut buf = vec![0u8; len as usize];
    reader.read_exact(&mut buf).await?;

    let frame: Frame = bincode::deserialize(&buf)?;
    Ok(Some(frame))
}
