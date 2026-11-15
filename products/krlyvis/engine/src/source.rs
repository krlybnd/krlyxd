use crate::{check_control, provider_error, SharedDrawing};
use krly3d_core::CallControl;
use krlyvis_api::{
    Diagnostic, DrawingCall, DrawingProvenance, DrawingQuality, DrawingTiming, ENGINE_REVISION,
};
use krlyvis_kernel::image::Gray;
use krlyvis_kernel::ports::outline::{outline, OutlineRequest};
use krlyvis_kernel::{Datum, Frame, Pixel, View, Vision, VisionBuilder};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A decoded image and its native provider services, reusable across selections.
///
/// The image is immutable and shared by Arc with every outline request. Encoded
/// bytes and request IDs are not retained. Each selection returns an independent
/// [`SharedDrawing`] that remains usable after this source is dropped.
pub struct DrawingSource {
    image: Arc<Gray>,
    vision: Vision,
    provenance: DrawingProvenance,
    read_ms: f64,
    read_total: Duration,
}

impl DrawingSource {
    /// Validate the call and decode its image exactly once using the existing readers.
    ///
    /// The call's seed is validated but does not select a component here; every
    /// [`Self::select`] supplies its own seed, with `None` choosing the largest.
    pub fn read(call: DrawingCall) -> Result<Self, Diagnostic> {
        Self::read_controlled(call, Arc::new(CallControl::new()))
    }

    /// Decode with per-call cooperative stopping. The returned source keeps no control.
    /// Hashing is checked in chunks; external codec decode is uninterruptible inside
    /// the decoder, with checks immediately before and after the provider call.
    pub fn read_controlled(call: DrawingCall, control: Arc<CallControl>) -> Result<Self, Diagnostic> {
        let started = Instant::now();
        check_control(&control)?;
        call.validate()?;
        check_control(&control)?;
        let input = call.input;
        let mut hash = Sha256::new();
        for chunk in input.image.data.chunks(64 * 1024) {
            check_control(&control)?;
            hash.update(chunk);
        }
        check_control(&control)?;
        let image_sha256 = format!("{:x}", hash.finalize());
        let mut builder = VisionBuilder::new();
        krlyvis_std::read::register(&mut builder);
        krlyvis_std::outline::register(&mut builder);
        let vision = builder.build();

        // The registered readers bound decoded dimensions before allocating samples.
        let reading = Instant::now();
        let cx = vision.context().with_call_control(Arc::clone(&control));
        let image = krlyvis_kernel::ports::read::read(&cx, &input.image.format, input.image.data)
            .into_result()
            .map_err(provider_error)?;
        check_control(&control)?;
        let read_ms = reading.elapsed().as_secs_f64() * 1000.0;
        let provenance = DrawingProvenance {
            image_sha256,
            source: input.source,
            image_format: input.image.format,
            image_size: [image.width(), image.height()],
            pixel_unit: input.pixel_unit,
            seed: None,
            method: "trace-holes".into(),
            simplify: input.simplify,
            engine_revision: ENGINE_REVISION.into(),
        };
        provenance.validate()?;
        let image = Arc::new(image);
        check_control(&control)?;
        Ok(Self { image, vision, provenance, read_ms, read_total: started.elapsed() })
    }

    /// Select one component and its holes using the original trace and frame providers.
    ///
    /// Seed validation uses the API's native provenance validator before tracing;
    /// the provider then rejects background, hole, and out-of-image seeds.
    /// A failed selection leaves this source available for further selections.
    ///
    /// The returned timing's `read_ms` is the original shared decode duration,
    /// and `outline_ms` measures only this selection's trace. `total_ms` includes
    /// the original source setup and this selection through frame construction,
    /// excluding idle time, earlier selections, and later validation/adaptation.
    pub fn select(&self, seed: Option<[f64; 2]>) -> Result<SharedDrawing, Diagnostic> {
        self.select_controlled(seed, Arc::new(CallControl::new()))
    }

    /// Select with a temporary controlled view of the shared providers. A stopped
    /// selection leaves this source and earlier evidence usable by independent calls.
    pub fn select_controlled(
        &self,
        seed: Option<[f64; 2]>,
        control: Arc<CallControl>,
    ) -> Result<SharedDrawing, Diagnostic> {
        let started = Instant::now();
        check_control(&control)?;
        let provenance = DrawingProvenance { seed, ..self.provenance.clone() };
        provenance.validate()?;
        let request = OutlineRequest {
            image: Arc::clone(&self.image),
            region: None,
            seed: seed.map(|[x, y]| Pixel::new(x, y)),
            threshold: None,
            simplify: provenance.simplify,
        };
        let cx = self.vision.context().with_call_control(Arc::clone(&control));
        let tracing = Instant::now();
        let shape = outline(&cx, "trace-holes", &request).into_result().map_err(provider_error)?;
        check_control(&control)?;
        let outline_ms = tracing.elapsed().as_secs_f64() * 1000.0;
        let unit = provenance.pixel_unit;
        let frame = Frame::through(
            View::Top,
            [0, 1],
            (Pixel::new(0.0, 0.0), [0.0, 0.0]),
            (Pixel::new(1.0, 1.0), [unit, unit]),
        )
        .map_err(provider_error)?;
        let datum = Datum::Placed { shape: Arc::new(shape), frame };
        check_control(&control)?;
        let drawing = SharedDrawing::new(
            datum,
            provenance,
            DrawingQuality::default(),
            DrawingTiming {
                read_ms: self.read_ms,
                outline_ms,
                total_ms: (self.read_total + started.elapsed()).as_secs_f64() * 1000.0,
            },
        )?;
        check_control(&control)?;
        Ok(drawing)
    }

    /// Image identity and caller claim, with no component selected (`seed: None`).
    pub fn provenance(&self) -> &DrawingProvenance {
        &self.provenance
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::DrawingSource;
    use krly3d_core::{Attempt, CallControl, Clock, Id};
    use krlyvis_api::{DrawingCall, DrawingImage, DrawingInput, PROTOCOL};
    use krlyvis_kernel::image::Gray;
    use krlyvis_kernel::ports::outline::{Outline, OutlineRequest, Shape};
    use krlyvis_kernel::{Context, Plugin, VisionBuilder};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, OnceLock, Weak};
    use std::time::Duration;

    struct ObserveImage {
        image: Weak<Gray>,
        calls: Arc<AtomicUsize>,
    }

    impl Plugin<Outline> for ObserveImage {
        fn id(&self) -> Id {
            Id::new("trace-holes")
        }

        fn call(&self, _: &Context, request: &OutlineRequest) -> Attempt<Shape> {
            assert!(Arc::ptr_eq(&self.image.upgrade().unwrap(), &request.image));
            self.calls.fetch_add(1, Ordering::Relaxed);
            Attempt::declined("image identity checked; continue to the existing tracer")
        }
    }

    #[test]
    fn selections_pass_the_original_image_arc_and_need_no_reader() {
        let mut bytes = b"P5\n6 6\n255\n".to_vec();
        for y in 0..6 {
            for x in 0..6 {
                bytes.push(if (1..5).contains(&x) && (1..5).contains(&y) { 40 } else { 230 });
            }
        }
        let mut source = DrawingSource::read(DrawingCall {
            protocol: PROTOCOL.into(),
            request_id: "discarded".into(),
            input: DrawingInput {
                image: DrawingImage { format: "pnm".into(), data: bytes },
                pixel_unit: 1.0,
                simplify: 0.0,
                seed: None,
                source: "arc-test".into(),
            },
        })
        .unwrap();
        let image = Arc::downgrade(&source.image);
        let calls = Arc::new(AtomicUsize::new(0));
        // Observe every actual provider request. No reader is registered in this
        // context, so selections must use the source's already decoded image.
        let mut builder = VisionBuilder::new();
        builder.plugin::<Outline>(ObserveImage { image: image.clone(), calls: Arc::clone(&calls) });
        krlyvis_std::outline::register(&mut builder);
        source.vision = builder.build();
        let first = source.select(None).unwrap();
        assert!(source.select(Some([0.0, 0.0])).is_err());
        let second = source.select(Some([2.5, 2.5])).unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 3);
        assert_eq!(Arc::strong_count(&source.image), 1);
        assert_eq!(first.polygons(), second.polygons());
        drop(source);
        assert!(image.upgrade().is_none());
        assert_eq!(first.material_area(), 16.0);
        assert_eq!(second.material_area(), 16.0);
    }

    /// Armed by the outline provider chain, so admission checks cannot consume
    /// this budget. The fifth check is inside the second histogram scan chunk.
    struct OutlineStop {
        armed: AtomicBool,
        checks: AtomicUsize,
        cancel: bool,
        control: OnceLock<Weak<CallControl>>,
    }

    impl Clock for OutlineStop {
        fn now(&self) -> Duration {
            if self.armed.load(Ordering::Relaxed) && self.checks.fetch_add(1, Ordering::Relaxed) + 1 >= 5 {
                if self.cancel {
                    self.control.get().unwrap().upgrade().unwrap().cancel();
                } else {
                    return Duration::from_secs(1);
                }
            }
            Duration::ZERO
        }
    }

    struct ArmOutline(Arc<OutlineStop>);

    impl Plugin<Outline> for ArmOutline {
        fn id(&self) -> Id {
            Id::new("trace-holes")
        }

        fn call(&self, _: &Context, _: &OutlineRequest) -> Attempt<Shape> {
            self.0.armed.store(true, Ordering::Relaxed);
            Attempt::declined("continue into the existing controlled tracer")
        }
    }

    #[test]
    fn stops_inside_real_outline_work_without_poisoning_source_or_retaining_control() {
        for cancel in [false, true] {
            let mut bytes = b"P5\n96 96\n255\n".to_vec();
            for y in 0..96 {
                for x in 0..96 {
                    let material = (4..88).contains(&x)
                        && (4..88).contains(&y)
                        && !((32..48).contains(&x) && (32..48).contains(&y));
                    bytes.push(if material { 40 } else { 230 });
                }
            }
            let mut source = DrawingSource::read(DrawingCall {
                protocol: PROTOCOL.into(),
                request_id: "outline-stop".into(),
                input: DrawingInput {
                    image: DrawingImage { format: "pnm".into(), data: bytes },
                    pixel_unit: 1.0,
                    simplify: 0.0,
                    seed: None,
                    source: "outline-control-test".into(),
                },
            })
            .unwrap();
            let before = source.select(None).unwrap();
            let before_result = before.to_result("before").unwrap();
            let clock = Arc::new(OutlineStop {
                armed: AtomicBool::new(false),
                checks: AtomicUsize::new(0),
                cancel,
                control: OnceLock::new(),
            });
            let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(1)));
            assert!(clock.control.set(Arc::downgrade(&control)).is_ok());
            let mut builder = VisionBuilder::new();
            builder.plugin::<Outline>(ArmOutline(Arc::clone(&clock)));
            krlyvis_std::outline::register(&mut builder);
            source.vision = builder.build();
            let error = source.select_controlled(None, Arc::clone(&control)).unwrap_err();
            assert_eq!(error.code.as_str(), if cancel { "cancelled" } else { "deadline_exceeded" });
            // Expiry fires within the scan. The context may observe that same
            // deadline once more while returning the provider's failure.
            assert!((5..=6).contains(&clock.checks.load(Ordering::Relaxed)));
            assert_eq!(Arc::strong_count(&control), 1);
            let weak = Arc::downgrade(&control);
            drop(control);
            assert!(weak.upgrade().is_none());
            for recovered in [
                source.select(None).unwrap(),
                source.select_controlled(Some([5.5, 5.5]), Arc::new(CallControl::new())).unwrap(),
            ] {
                assert_eq!(recovered.polygons(), before.polygons());
                assert_eq!(recovered.material_area(), 6800.0);
                assert_eq!(recovered.quality(), before.quality());
                assert_eq!(recovered.timing().read_ms, before.timing().read_ms);
            }
            assert_eq!(before.to_result("before").unwrap(), before_result);
            assert_eq!(source.provenance().seed, None);
        }
    }
}
