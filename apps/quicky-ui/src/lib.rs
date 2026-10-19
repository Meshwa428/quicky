//! Shared pieces for the Quicky editor apps: the compact window frame, the
//! Omarchy theme, an embedded mpv player, a timeline strip, an image canvas
//! with boxes, and media helpers.

pub mod app;
pub mod canvas;
pub mod deck;
pub mod draw;
pub mod frame;
pub mod media;
pub mod player;
pub mod preview;
pub mod theme;
pub mod timeline;

pub use canvas::Canvas;
pub use deck::Deck;
pub use frame::Frame;
pub use player::Player;
pub use preview::Preview;
pub use theme::Palette;
pub use timeline::Timeline;
