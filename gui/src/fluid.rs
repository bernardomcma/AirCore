//! Self-contained 60/120Hz spring-animated Apple-style widgets for `libcosmic` / `iced` 0.14.

pub mod pressable;
pub mod segmented;
pub mod slider;
pub mod switch;
pub mod transition;

pub use pressable::fluid_pressable;
pub use segmented::fluid_segmented_control;
pub use slider::fluid_slider;
pub use switch::fluid_switch;
pub use transition::fluid_transition;
