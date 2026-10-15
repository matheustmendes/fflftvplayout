use ffmpeg_next::frame;
use log::debug;

use crate::{
    compositor::{
        logo::{LogoOverlay, blend_logo},
        text::TextOverlay,
    },
    utils::config::{GraphicContent, GraphicLayer, GraphicLayersState, OutputConfig, TextConfig},
};

enum LayerOverlay {
    Text(TextOverlay),
    Image(LogoOverlay, ImageFade),
}

/// Fade of an image layer, in output frames. Text layers fade inside
/// `TextOverlay`.
struct ImageFade {
    in_frames: i64,
    out_frames: i64,
    start_pts: i64,
    end_pts: Option<i64>,
}

fn seconds_to_frames(seconds: f64, fps: u32) -> i64 {
    if seconds.is_finite() && seconds > 0.0 {
        (seconds * f64::from(fps)).round().max(1.0) as i64
    } else {
        0
    }
}

impl ImageFade {
    /// How visible the image is at `pts`: 0 at the start of a fade in, 1 in
    /// the middle and 0 at the end of a fade out.
    fn factor(&self, pts: i64) -> f64 {
        let mut factor: f64 = 1.0;

        if self.in_frames > 0 {
            factor = factor.min((pts - self.start_pts).max(0) as f64 / self.in_frames as f64);
        }

        if self.out_frames > 0
            && let Some(end) = self.end_pts
        {
            factor = factor.min((end - pts).max(0) as f64 / self.out_frames as f64);
        }

        factor.clamp(0.0, 1.0)
    }
}

struct LoadedLayer {
    layer: GraphicLayer,
    overlay: LayerOverlay,
}

/// Per-decoder view of [`GraphicLayersState`]: keeps the rendered overlays and
/// reloads only the layers that appeared or changed since the last frame.
pub(crate) struct RuntimeGraphicLayers {
    state: GraphicLayersState,
    revision: u64,
    loaded: Vec<LoadedLayer>,
    width: u32,
    height: u32,
    fps: u32,
    channel_id: i32,
}

impl RuntimeGraphicLayers {
    pub(crate) fn new(cfg: &OutputConfig, pts: i64, scroll_pts: i64) -> Self {
        let mut layers = Self {
            state: cfg.graphic_layers.clone(),
            // Differs from any real revision, so the first update loads everything.
            revision: u64::MAX,
            loaded: Vec::new(),
            width: cfg.width,
            height: cfg.height,
            fps: cfg.fps,
            channel_id: cfg.channel_id.unwrap_or_default(),
        };

        layers.update(pts, scroll_pts);

        layers
    }

    pub(crate) fn is_active(&self) -> bool {
        !self.loaded.is_empty()
    }

    pub(crate) fn update(&mut self, pts: i64, scroll_pts: i64) {
        let revision = self.state.revision();

        if revision == self.revision {
            return;
        }

        self.revision = revision;

        let mut previous = std::mem::take(&mut self.loaded);

        for layer in self.state.layers() {
            let kept = previous
                .iter()
                .position(|loaded| loaded.layer == layer)
                .map(|index| previous.swap_remove(index));

            if let Some(kept) = kept {
                self.loaded.push(kept);
                continue;
            }

            match self.load(&layer, pts, scroll_pts) {
                Ok(overlay) => self.loaded.push(LoadedLayer { layer, overlay }),
                Err(error) => {
                    debug!(channel = self.channel_id; "graphic layer {} not drawn: {error:#}", layer.id);
                }
            }
        }
    }

    pub(crate) fn blend(&mut self, frame: &mut frame::Video, pts: i64, scroll_pts: i64) {
        for loaded in &mut self.loaded {
            match &mut loaded.overlay {
                LayerOverlay::Text(text) => text.blend(frame, pts, scroll_pts),
                LayerOverlay::Image(image, fade) => blend_logo(frame, image, fade.factor(pts)),
            }
        }
    }

    fn load(
        &self,
        layer: &GraphicLayer,
        pts: i64,
        scroll_pts: i64,
    ) -> anyhow::Result<LayerOverlay> {
        let end_pts = layer
            .visible_seconds
            .map(|seconds| pts + seconds_to_frames(seconds, self.fps));

        match &layer.content {
            GraphicContent::Text(text) => {
                let text = TextConfig {
                    fade_in_seconds: layer.fade_in_seconds,
                    fade_out_seconds: layer.fade_out_seconds,
                    ..text.clone()
                };
                let overlay = TextOverlay::load(
                    &text,
                    "",
                    self.width,
                    self.height,
                    self.fps,
                    pts,
                    scroll_pts,
                    end_pts,
                )?;

                overlay
                    .map(LayerOverlay::Text)
                    .ok_or_else(|| anyhow::anyhow!("the text is empty"))
            }
            GraphicContent::Image(image) => {
                let fade = ImageFade {
                    in_frames: seconds_to_frames(layer.fade_in_seconds, self.fps),
                    out_frames: seconds_to_frames(layer.fade_out_seconds, self.fps),
                    start_pts: pts,
                    end_pts,
                };

                LogoOverlay::load(image, self.width, self.height)
                    .map(|overlay| LayerOverlay::Image(overlay, fade))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use ffmpeg_next::util::format::pixel::Pixel;

    use super::*;
    use crate::utils::config::{LogoConfig, TextConfig, TextPosition};

    const WIDTH: u32 = 640;
    const HEIGHT: u32 = 360;

    fn black_frame() -> frame::Video {
        let mut frame = frame::Video::new(Pixel::YUV420P, WIDTH, HEIGHT);
        let stride = frame.stride(0);

        for row in 0..HEIGHT as usize {
            frame.data_mut(0)[row * stride..row * stride + WIDTH as usize].fill(16);
        }

        frame
    }

    /// Bright luma pixels inside the box, to tell where a layer was drawn.
    fn bright_pixels(
        frame: &frame::Video,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
    ) -> usize {
        let stride = frame.stride(0);

        (y..y + height)
            .flat_map(|row| (x..x + width).map(move |col| (row, col)))
            .filter(|(row, col)| frame.data(0)[row * stride + col] > 40)
            .count()
    }

    fn layers_for(state: &GraphicLayersState) -> RuntimeGraphicLayers {
        let cfg = OutputConfig::new(WIDTH, HEIGHT, 25, 48_000).with_graphic_layers(state.clone());

        RuntimeGraphicLayers::new(&cfg, 0, 0)
    }

    #[test]
    fn a_text_layer_is_drawn_at_its_exact_position_and_removed_when_cleared() {
        crate::compositor::text::init();

        let state = GraphicLayersState::default();

        state.set(vec![GraphicLayer::new(
            1,
            GraphicContent::Text(TextConfig {
                text: Some("WWWW".to_string()),
                font_size: 40.0,
                position_x: TextPosition::Pixels(320),
                position_y: TextPosition::Pixels(200),
                ..TextConfig::default()
            }),
        )]);

        let mut layers = layers_for(&state);
        let mut frame = black_frame();

        assert!(layers.is_active());
        layers.blend(&mut frame, 0, 0);

        assert!(bright_pixels(&frame, 320, 200, 200, 60) > 50);
        assert_eq!(bright_pixels(&frame, 0, 0, 300, 180), 0);

        state.set(Vec::new());
        layers.update(1, 1);

        let mut frame = black_frame();

        layers.blend(&mut frame, 1, 1);
        assert!(!layers.is_active());
        assert_eq!(
            bright_pixels(&frame, 0, 0, WIDTH as usize, HEIGHT as usize),
            0
        );
    }

    #[test]
    fn an_image_layer_is_scaled_and_placed_and_a_missing_file_is_skipped() {
        let logo =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests_assets/storage/logo.png");
        let state = GraphicLayersState::default();

        state.set(vec![
            GraphicLayer::new(
                1,
                GraphicContent::Image(LogoConfig {
                    path: logo.to_string_lossy().to_string(),
                    scale: Some("100:-1".to_string()),
                    opacity: 1.0,
                    position: "400:20".to_string(),
                }),
            ),
            GraphicLayer::new(
                2,
                GraphicContent::Image(LogoConfig {
                    path: "/nonexistent/none.png".to_string(),
                    scale: None,
                    opacity: 1.0,
                    position: "0:0".to_string(),
                }),
            ),
        ]);

        let mut layers = layers_for(&state);
        let mut frame = black_frame();

        assert!(layers.is_active());
        assert_eq!(layers.loaded.len(), 1);
        layers.blend(&mut frame, 0, 0);

        assert!(bright_pixels(&frame, 400, 20, 100, 100) > 100);
        assert_eq!(bright_pixels(&frame, 0, 0, 380, HEIGHT as usize), 0);
    }

    #[test]
    fn an_image_fades_in_and_out_around_its_time_on_the_air() {
        let fade = ImageFade {
            in_frames: 10,
            out_frames: 10,
            start_pts: 100,
            end_pts: Some(200),
        };

        assert_eq!(fade.factor(100), 0.0);
        assert!((fade.factor(105) - 0.5).abs() < 1e-9);
        assert_eq!(fade.factor(150), 1.0);
        assert!((fade.factor(195) - 0.5).abs() < 1e-9);
        assert_eq!(fade.factor(200), 0.0);
        assert_eq!(fade.factor(250), 0.0);

        let unknown_end = ImageFade {
            out_frames: 10,
            end_pts: None,
            ..fade
        };

        assert_eq!(unknown_end.factor(10_000), 1.0, "no end, no fade out");
    }

    #[test]
    fn a_fading_text_layer_is_invisible_at_the_start_and_gone_at_the_end() {
        crate::compositor::text::init();

        let state = GraphicLayersState::default();
        let mut layer = GraphicLayer::new(
            1,
            GraphicContent::Text(TextConfig {
                text: Some("WWWW".to_string()),
                font_size: 40.0,
                position_x: TextPosition::Pixels(100),
                position_y: TextPosition::Pixels(100),
                ..TextConfig::default()
            }),
        );

        layer.fade_in_seconds = 1.0;
        layer.fade_out_seconds = 1.0;
        layer.visible_seconds = Some(4.0);
        state.set(vec![layer]);

        let mut layers = layers_for(&state);
        let mut bright = |pts: i64| {
            let mut frame = black_frame();

            layers.blend(&mut frame, pts, pts);

            bright_pixels(&frame, 100, 100, 250, 60)
        };

        assert_eq!(bright(0), 0, "starts invisible");
        assert!(
            bright(50) > 50,
            "visible in the middle (frame 50 at 25 fps)"
        );
        assert_eq!(bright(100), 0, "gone when the time on the air ends");
    }

    #[test]
    fn scrolling_text_moves_across_the_picture() {
        crate::compositor::text::init();

        let state = GraphicLayersState::default();

        state.set(vec![GraphicLayer::new(
            1,
            GraphicContent::Text(TextConfig {
                text: Some("WWWW".to_string()),
                font_size: 40.0,
                position_y: TextPosition::Pixels(100),
                scroll: crate::utils::config::TextScroll::RightToLeft {
                    pixels_per_second: 250,
                },
                ..TextConfig::default()
            }),
        )]);

        let mut layers = layers_for(&state);
        let mut at = |pts: i64| {
            let mut frame = black_frame();

            layers.blend(&mut frame, pts, pts);

            (
                bright_pixels(&frame, 0, 100, 320, 60),
                bright_pixels(&frame, 320, 100, 320, 60),
            )
        };

        // At 25 fps the text moves 10 px per frame, so frame 20 is 200 px in, starting beyond the right edge.
        assert_eq!(at(0), (0, 0));
        let (left, right) = at(20);

        assert!(
            right > 0 && left == 0,
            "enters from the right: {left} {right}"
        );
        let (left, _) = at(60);

        assert!(left > 0, "crosses to the left half");
    }
}
