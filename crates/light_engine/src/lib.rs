pub mod plan;
pub mod effects;
pub mod scheduler;
pub mod color;

pub use plan::{LightPlan, Palette};
pub use effects::{Effect, EffectKind, Trigger};
pub use scheduler::{EffectScheduler, EffectContext, RenderFrame};
pub use color::Rgb;

/// Number of LEDs in the strip — matches the existing hardware.
pub const LED_COUNT: usize = 259;
