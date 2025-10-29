use super::error::ProtocolError;
use async_trait::async_trait;
use bytes::Bytes;

#[derive(Debug, Clone)]
pub enum FrameType {
    H2(FrameTypeH2),
    H3(FrameTypeH3),
}

#[derive(Debug, Clone)]
pub enum FrameTypeH2 {
    Data,         // 0x0
    Headers,      // 0x1
    Priority,     // 0x2
    RstStream,    // 0x3
    Settings,     // 0x4
    PushPromise,  // 0x5
    Ping,         // 0x6
    GoAway,       // 0x7
    WindowUpdate, // 0x8
    Continuation, // 0x9
}

#[derive(Debug, Clone)]
pub enum FrameTypeH3 {
    Data,        // 0x0
    Headers,     // 0x1
    CancelPush,  // 0x3
    Settings,    // 0x4
    PushPromise, // 0x5
    GoAway,      // 0x7
    MaxPushId,   // 0xd
    Unknown(u64),
}

#[derive(Debug, Clone)]
pub struct FrameH2 {
    pub frame_type: FrameType,
    pub flags: u8,
    pub stream_id: u32,
    pub payload: Bytes,
}

#[derive(Debug, Clone)]
pub struct FrameH3 {
    pub frame_type: FrameType,
    pub stream_id: u32,
    pub payload: Bytes,
}

#[async_trait(?Send)]
pub trait FrameSink<F> {
    async fn write_frame(&mut self, frame: F) -> Result<(), ProtocolError>;
}

pub trait IntoFrameBatch<F> {
    fn into_batch(self) -> FrameBatch<F>;
}

impl<F> IntoFrameBatch<F> for FrameBatch<F> {
    fn into_batch(self) -> FrameBatch<F> {
        self
    }
}

impl<F> IntoFrameBatch<F> for F {
    fn into_batch(self) -> FrameBatch<F> {
        FrameBatch::new(vec![self])
    }
}

pub trait FrameBuilderExt<F>: Sized {
    fn repeat(self, count: usize) -> FrameBatch<F>;
    fn chain(self, other: impl IntoFrameBatch<F>) -> FrameBatch<F>;
    fn into_batch(self) -> FrameBatch<F>;
}

impl<F: Clone> FrameBuilderExt<F> for F {
    fn repeat(self, count: usize) -> FrameBatch<F> {
        FrameBatch {
            frames: vec![self; count],
        }
    }

    fn chain(self, other: impl IntoFrameBatch<F>) -> FrameBatch<F> {
        let mut batch = FrameBatch::new(vec![self]);
        batch.extend(other.into_batch());
        batch
    }

    fn into_batch(self) -> FrameBatch<F> {
        FrameBatch::new(vec![self])
    }
}

#[derive(Debug, Clone)]
pub struct FrameBatch<F> {
    frames: Vec<F>,
}

impl<F> IntoIterator for FrameBatch<F> {
    type Item = F;
    type IntoIter = std::vec::IntoIter<F>;

    fn into_iter(self) -> Self::IntoIter {
        self.frames.into_iter()
    }
}

impl<F> FrameBatch<F> {
    pub fn new(frames: Vec<F>) -> Self {
        Self { frames }
    }

    pub fn push(&mut self, frame: F) {
        self.frames.push(frame);
    }

    pub fn extend<I: IntoIterator<Item = F>>(&mut self, iter: I) {
        self.frames.extend(iter);
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn frames(&self) -> &[F] {
        &self.frames
    }

    pub fn into_frames(self) -> Vec<F> {
        self.frames
    }

    pub fn map<G, M>(self, mut map_fn: M) -> FrameBatch<G>
    where
        M: FnMut(F) -> G,
    {
        FrameBatch {
            frames: self.frames.into_iter().map(|f| map_fn(f)).collect(),
        }
    }

    pub fn chain(mut self, other: impl IntoFrameBatch<F>) -> FrameBatch<F> {
        let mut other = other.into_batch();
        self.frames.append(&mut other.frames);
        self
    }

    pub async fn send<S>(self, sink: &mut S) -> Result<(), ProtocolError>
    where
        S: FrameSink<F> + ?Sized,
    {
        for frame in self.frames {
            sink.write_frame(frame).await?;
        }
        Ok(())
    }
}
