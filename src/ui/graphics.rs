//! Terminal graphics capability detection for in-terminal image rendering.
//!
//! Wraps `ratatui-image`'s [`Picker`]: the terminal is queried over stdio
//! once (lazily, on first use) for its graphics protocol — kitty, iTerm2, or
//! sixel — and its font size; terminals without pixel graphics fall back to
//! halfblocks. The query needs a real terminal in raw mode, so `Reader`
//! instances built for tests use [`Graphics::disabled`] and never probe.

use image::DynamicImage;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::StatefulProtocol;

pub struct Graphics {
    picker: PickerState,
}

enum PickerState {
    /// Terminal not queried yet; probed on first use.
    Unprobed,
    Available(Picker),
    Unavailable,
}

impl Graphics {
    /// Graphics that will probe the terminal on first use.
    pub fn new() -> Self {
        Self {
            picker: PickerState::Unprobed,
        }
    }

    /// Graphics that never probe and never render (tests, non-terminal backends).
    pub fn disabled() -> Self {
        Self {
            picker: PickerState::Unavailable,
        }
    }

    /// Graphics with a fixed halfblocks picker, so snapshot tests can render
    /// the image viewer without querying a real terminal.
    #[cfg(test)]
    pub fn halfblocks_for_test() -> Self {
        Self {
            picker: PickerState::Available(Picker::halfblocks()),
        }
    }

    fn picker(&mut self) -> Option<&Picker> {
        if matches!(self.picker, PickerState::Unprobed) {
            self.picker = match Picker::from_query_stdio() {
                Ok(picker) => PickerState::Available(picker),
                Err(_) => PickerState::Unavailable,
            };
        }
        match &self.picker {
            PickerState::Available(picker) => Some(picker),
            _ => None,
        }
    }

    /// Whether the terminal can render images (probes on first call).
    pub fn is_available(&mut self) -> bool {
        self.picker().is_some()
    }

    /// Human-readable name of the detected protocol, for status messages.
    pub fn protocol_name(&mut self) -> Option<&'static str> {
        self.picker().map(|p| match p.protocol_type() {
            ProtocolType::Kitty => "kitty",
            ProtocolType::Iterm2 => "iTerm2",
            ProtocolType::Sixel => "sixel",
            ProtocolType::Halfblocks => "halfblocks",
        })
    }

    /// Build a render protocol for a decoded image, or `None` when the
    /// terminal query failed and in-terminal rendering is unavailable.
    pub fn new_protocol(&mut self, image: DynamicImage) -> Option<StatefulProtocol> {
        self.picker()
            .map(|picker| picker.new_resize_protocol(image))
    }
}

impl Default for Graphics {
    fn default() -> Self {
        Self::new()
    }
}
