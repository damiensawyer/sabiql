use std::sync::Arc;
use std::time::Instant;

use crate::model::app_state::AppState;
use crate::services::AppServices;

pub use crate::model::shared::render_output::RenderOutput;

#[derive(Debug, Clone, thiserror::Error)]
pub enum RenderError {
    #[error("I/O error: {0}")]
    Io(#[source] Arc<std::io::Error>),
}

impl From<std::io::Error> for RenderError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(Arc::new(error))
    }
}

pub type RenderResult<T> = Result<T, RenderError>;

pub trait Renderer {
    fn draw(
        &mut self,
        state: &AppState,
        services: &AppServices,
        now: Instant,
    ) -> RenderResult<RenderOutput>;

    /// Give up exclusive control of the terminal (raw mode, alternate
    /// screen, background input reader) so an external process can take
    /// over. No-op by default; real terminal renderers must override this.
    fn suspend(&mut self) -> RenderResult<()> {
        Ok(())
    }

    /// Reclaim exclusive control of the terminal after [`Renderer::suspend`]
    /// and force a full repaint, since the physical screen content is no
    /// longer known to match whatever the renderer last drew.
    fn resume(&mut self) -> RenderResult<()> {
        Ok(())
    }
}
