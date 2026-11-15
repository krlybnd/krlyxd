//! CLI parsing and bounded file input; native validation stays at the host boundary.

use krly3d_reconstruction::Settings;
use krlyvis_api::{DrawingCall, DrawingImage, DrawingInput, MAX_IMAGE_BYTES, MAX_SIMPLIFY_PIXELS, PROTOCOL};
use krlyxd::CallControl;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

pub(super) const HELP: &str = "krly3d-reconstruct --image PATH --output NEW_DIR --pixel-unit NUMBER --height NUMBER [--seed X,Y] [--tolerance NUMBER] [--outline-tolerance PIXELS] [--timeout SECONDS] [--stream]\nkrly3d-reconstruct --image PATH --output NEW_DIR --pixel-unit NUMBER --parts PATH [--tolerance NUMBER] [--outline-tolerance PIXELS] [--timeout SECONDS]\nSingle-process planar reconstruction; top view, supplied scale and uniform thickness. No inferred hidden geometry. Output: evidence.json, proposal.k3d, mesh.json, manifest.json, events.jsonl. With --stream: flushed NDJSON stages plus coarse-mesh.json, preview.png and likeness.json. Sheet mode always emits flushed NDJSON, saves parts/<id>/ artifacts and a retained manifest; --height and --seed are incompatible. Outline tolerance defaults to 0 (exact trace); opt-in 0..1024 source-image pixels, independent of model scale and mesh tolerance. Unsafe simplification may retain exact loops; physical accuracy remains unknown. Timeout defaults to 120 seconds in every mode. SIGINT/SIGTERM request cancellation. Unix pipe output is interruptible; decoder/provider and filesystem calls may still finish before a checkpoint. Inputs must be regular files.";

pub(super) struct SheetOptions {
    pub(super) parts: PathBuf,
}

pub(super) struct Options {
    pub(super) timeout: Duration,
    pub(super) output: PathBuf,
    pub(super) settings: Settings,
    pub(super) stream: bool,
    pub(super) sheet: Option<SheetOptions>,
    image: PathBuf,
    format: &'static str,
    pixel_unit: f64,
    simplify: f64,
    seed: Option<[f64; 2]>,
}

impl Options {
    /// `None` requests help, including the existing early return during parsing.
    pub(super) fn parse(mut arguments: impl Iterator<Item = String>) -> Result<Option<Self>, String> {
        let mut options = BTreeMap::new();
        let mut stream = false;
        while let Some(name) = arguments.next() {
            if name == "--stream" {
                if stream {
                    return Err("duplicate --stream".into());
                }
                stream = true;
                continue;
            }
            if name == "--help" {
                return Ok(None);
            }
            if ![
                "--image",
                "--output",
                "--pixel-unit",
                "--height",
                "--seed",
                "--tolerance",
                "--outline-tolerance",
                "--parts",
                "--timeout",
            ]
            .contains(&name.as_str())
            {
                return Err(format!("unknown option {name}"));
            }
            let value = arguments.next().ok_or_else(|| format!("missing value for {name}"))?;
            if options.insert(name.clone(), value).is_some() {
                return Err(format!("duplicate option {name}"));
            }
        }
        let required = |name: &str| options.get(name).ok_or_else(|| format!("missing {name}"));
        let image = PathBuf::from(required("--image")?);
        let output = PathBuf::from(required("--output")?);
        if output.exists() {
            return Err("output directory already exists".into());
        }
        let number =
            |name: &str| required(name)?.parse::<f64>().map_err(|_| format!("invalid number for {name}"));
        let pixel_unit = number("--pixel-unit")?;
        let simplify = options.get("--outline-tolerance").map_or(Ok(0.0), |value| {
            value.parse::<f64>().map_err(|_| "invalid outline tolerance".to_string())
        })?;
        if !simplify.is_finite() || !(0.0..=MAX_SIMPLIFY_PIXELS).contains(&simplify) {
            return Err("outline tolerance must be finite and between 0 and 1024 source-image pixels".into());
        }
        let sheet = if let Some(path) = options.get("--parts") {
            if options.contains_key("--height") || options.contains_key("--seed") {
                return Err("--parts is incompatible with --height and --seed".into());
            }
            Some(SheetOptions { parts: PathBuf::from(path) })
        } else {
            None
        };
        let seconds = options
            .get("--timeout")
            .map_or(Ok(120.0), |value| value.parse::<f64>().map_err(|_| "invalid timeout".to_string()))?;
        let timeout = Duration::try_from_secs_f64(seconds)
            .ok()
            .filter(|duration| !duration.is_zero())
            .ok_or("timeout must be positive, finite and representable in seconds")?;
        // Sheet heights belong to the individual typed part specs.
        let height = if sheet.is_some() { 1.0 } else { number("--height")? };
        let tolerance = options
            .get("--tolerance")
            .map_or(Ok(0.05), |value| value.parse::<f64>().map_err(|_| "invalid tolerance".to_string()))?;
        let seed = options
            .get("--seed")
            .map(|value| {
                let (x, y) = value.split_once(',').ok_or("seed must be X,Y")?;
                Ok::<_, String>([
                    x.parse().map_err(|_| "invalid seed x")?,
                    y.parse().map_err(|_| "invalid seed y")?,
                ])
            })
            .transpose()?;
        let format = match image.extension().and_then(|s| s.to_str()).map(str::to_ascii_lowercase).as_deref()
        {
            Some("png") => "png",
            Some("jpg" | "jpeg") => "jpeg",
            Some("pnm" | "pgm" | "ppm") => "pnm",
            _ => return Err("image must be PNG, JPEG or PNM".into()),
        };
        Ok(Some(Self {
            timeout,
            output,
            settings: Settings { height, tolerance },
            stream,
            sheet,
            image,
            format,
            pixel_unit,
            simplify,
            seed,
        }))
    }

    pub(super) fn read_call(&self, control: &CallControl) -> Result<DrawingCall, String> {
        let data = super::io_control::read_file(&self.image, MAX_IMAGE_BYTES, control)?;
        Ok(DrawingCall {
            protocol: PROTOCOL.into(),
            request_id: "local-drawing".into(),
            input: DrawingInput {
                image: DrawingImage { format: self.format.into(), data },
                pixel_unit: self.pixel_unit,
                simplify: self.simplify,
                seed: self.seed,
                source: self.image.display().to_string(),
            },
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{Options, HELP};
    use krlyxd::CallControl;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn arguments(extra: &[&str]) -> Vec<String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let output = std::env::temp_dir().join(format!(
            "krly3d-outline-options-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut args = vec![
            "--image".into(),
            "outline.pgm".into(),
            "--output".into(),
            output.display().to_string(),
            "--pixel-unit".into(),
            "0.25".into(),
            "--height".into(),
            "3".into(),
        ];
        args.extend(extra.iter().map(|value| (*value).to_owned()));
        args
    }

    #[test]
    fn outline_tolerance_defaults_to_exact_and_is_independent_of_model_units() {
        let default = Options::parse(arguments(&[]).into_iter()).unwrap().unwrap();
        assert_eq!(default.simplify, 0.0);
        let positive = Options::parse(arguments(&["--outline-tolerance", "1"]).into_iter()).unwrap().unwrap();
        assert_eq!(positive.simplify, 1.0);
        assert_eq!(positive.pixel_unit, 0.25);
        assert_eq!(positive.settings.tolerance, 0.05);
        assert!(HELP.contains("--outline-tolerance PIXELS"));
        assert!(HELP.contains("0 (exact trace)"));
    }

    #[test]
    fn invalid_outline_tolerances_are_rejected_before_file_io() {
        for value in ["NaN", "inf", "-inf", "-1", "1025", "not-a-number"] {
            assert!(
                Options::parse(arguments(&["--outline-tolerance", value]).into_iter()).is_err(),
                "{value}"
            );
        }
        assert!(Options::parse(arguments(&["--outline-tolerance"]).into_iter()).is_err());
        assert!(Options::parse(
            arguments(&["--outline-tolerance", "1", "--outline-tolerance", "2"]).into_iter()
        )
        .is_err());
    }

    #[test]
    fn requested_outline_tolerance_is_forwarded_to_the_drawing_call() {
        let mut options =
            Options::parse(arguments(&["--outline-tolerance", "1"]).into_iter()).unwrap().unwrap();
        let path = options.output.with_extension("pgm");
        std::fs::write(&path, b"P5 1 1 255\n\0").unwrap();
        options.image = path.clone();
        let result = options.read_call(&CallControl::new());
        std::fs::remove_file(path).unwrap();
        let call = result.unwrap();
        assert_eq!(call.input.simplify, 1.0);
        call.validate().unwrap();
    }
}
