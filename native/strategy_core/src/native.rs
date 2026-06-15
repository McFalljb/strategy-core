use std::{collections::BTreeMap, error::Error, fmt, future::Future};

use serde::{Deserialize, Serialize};

use crate::{JsonValue, StrategyConfig, StrategyContext, kernel::NativeKernel};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeKernelStatus {
    Completed,
    FallbackCompleted,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NativeKernelResult {
    #[serde(default = "default_native_kernel_status")]
    pub status: NativeKernelStatus,
    #[serde(default)]
    pub events_handled: i64,
    #[serde(default)]
    pub actions_emitted: i64,
    #[serde(default)]
    pub fallback_used: bool,
    #[serde(default)]
    pub metadata: BTreeMap<String, JsonValue>,
}

impl Default for NativeKernelResult {
    fn default() -> Self {
        Self {
            status: NativeKernelStatus::Completed,
            events_handled: 0,
            actions_emitted: 0,
            fallback_used: false,
            metadata: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeKernelUnavailableError {
    message: String,
}

impl NativeKernelUnavailableError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for NativeKernelUnavailableError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for NativeKernelUnavailableError {}

pub type NativeKernelUnavailable = NativeKernelUnavailableError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeKernelRunError<E> {
    Unavailable(NativeKernelUnavailableError),
    Runner(E),
}

impl<E> NativeKernelRunError<E> {
    #[must_use]
    pub const fn unavailable(error: NativeKernelUnavailableError) -> Self {
        Self::Unavailable(error)
    }

    #[must_use]
    pub const fn runner(error: E) -> Self {
        Self::Runner(error)
    }
}

impl<E> fmt::Display for NativeKernelRunError<E>
where
    E: fmt::Display,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(error) => error.fmt(formatter),
            Self::Runner(error) => write!(formatter, "native strategy kernel failed: {error}"),
        }
    }
}

impl<E> Error for NativeKernelRunError<E> where E: Error + 'static {}

impl<E> From<NativeKernelUnavailableError> for NativeKernelRunError<E> {
    fn from(error: NativeKernelUnavailableError) -> Self {
        Self::Unavailable(error)
    }
}

pub trait NativeKernelFactory<K: NativeKernel> {
    fn build(&self, config: &StrategyConfig) -> K;
}

pub trait NativeKernelRunner<K: NativeKernel> {
    type Error;

    fn run_native_kernel(
        &mut self,
        kernel: &mut K,
    ) -> impl Future<Output = Result<NativeKernelResult, Self::Error>> + Send;
}

pub trait NativeStrategyContext: StrategyContext {
    type NativeRunner;

    fn native_kernel_runner(&mut self) -> Option<&mut Self::NativeRunner>;
}

pub fn get_native_kernel_runner<C>(ctx: &mut C) -> Option<&mut C::NativeRunner>
where
    C: NativeStrategyContext,
{
    if !ctx.capabilities().supports_native_kernels {
        return None;
    }
    ctx.native_kernel_runner()
}

pub async fn run_native_or_fallback<C, K, F, Fut, E>(
    ctx: &mut C,
    kernel: &mut K,
    fallback: Option<F>,
    require_native: bool,
) -> Result<NativeKernelResult, NativeKernelRunError<E>>
where
    C: NativeStrategyContext,
    C::NativeRunner: NativeKernelRunner<K, Error = E>,
    K: NativeKernel,
    F: FnOnce() -> Fut,
    Fut: Future<Output = ()>,
    E: fmt::Display,
{
    if let Some(runner) = get_native_kernel_runner(ctx) {
        return runner
            .run_native_kernel(kernel)
            .await
            .map_err(NativeKernelRunError::Runner);
    }

    if require_native {
        return Err(NativeKernelRunError::Unavailable(
            NativeKernelUnavailableError::new(format!(
                "native strategy kernel {:?} was required, but this runtime does not support native kernels",
                kernel.name()
            )),
        ));
    }

    let Some(fallback) = fallback else {
        return Err(NativeKernelRunError::Unavailable(
            NativeKernelUnavailableError::new(format!(
                "native strategy kernel {:?} is unavailable and no fallback was provided",
                kernel.name()
            )),
        ));
    };

    fallback().await;
    Ok(NativeKernelResult {
        status: NativeKernelStatus::FallbackCompleted,
        fallback_used: true,
        ..NativeKernelResult::default()
    })
}

fn default_native_kernel_status() -> NativeKernelStatus {
    NativeKernelStatus::Completed
}
