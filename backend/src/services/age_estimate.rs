//! On-server age estimation for the 18+ queue (pre-sorting hint only).
//!
//! Two small ONNX models run locally with `tract` (pure Rust, no network,
//! no third party ever sees the face):
//!   * UltraFace RFB-320 — face detector (MIT, ONNX model zoo)
//!   * age_googlenet — Levi/Hassner 8-bucket age classifier (Apache-2.0, ONNX
//!     model zoo)
//!
//! The result is a *hint* for the human reviewer, never a decision: the
//! bucket boundaries (15-20 / 25-32) straddle 18, so only a clear majority
//! on the 25+ buckets is reported as "likely adult", a clear majority on
//! the child buckets as "likely minor", everything else as "uncertain".
//! Runs only with the user's explicit consent (DSGVO Art. 9(2)(a)); the
//! feature is off when the model files are missing.
use std::path::PathBuf;
use std::sync::OnceLock;
use tract_onnx::prelude::*;

pub const MODEL_TAG: &str = "ultraface-rfb-320@34cd7e60 + age_googlenet@fa2a3228";
const BUCKETS: [&str; 8] = ["0-2", "4-6", "8-12", "15-20", "25-32", "38-43", "48-53", "60-100"];
const FACE_SCORE_MIN: f32 = 0.7;
const ADULT_MIN: f32 = 0.85;
const CHILD_MIN: f32 = 0.5;

type Model = std::sync::Arc<TypedRunnableModel>;

struct Models { detector: Model, age: Model }

static MODELS: OnceLock<Option<Models>> = OnceLock::new();

fn model_dir() -> PathBuf {
    PathBuf::from(std::env::var("AGE_AI_MODEL_DIR").unwrap_or_else(|_| "/app/models".into()))
}

fn enabled() -> bool {
    !matches!(std::env::var("AGE_AI_ENABLED").as_deref(), Ok("false") | Ok("0") | Ok("off"))
}

fn load() -> Option<Models> {
    if !enabled() { return None; }
    let dir = model_dir();
    let det_path = dir.join("version-RFB-320.onnx");
    let age_path = dir.join("age_googlenet.onnx");
    if !det_path.is_file() || !age_path.is_file() {
        tracing::info!("age-ai: model files not found in {} — pre-sorting disabled", dir.display());
        return None;
    }
    let build = |p: &PathBuf, h: usize, w: usize| -> TractResult<Model> {
        tract_onnx::onnx()
            .model_for_path(p)?
            .with_input_fact(0, f32::fact([1, 3, h, w]).into())?
            .into_optimized()?
            .into_runnable()
    };
    match (build(&det_path, 240, 320), build(&age_path, 224, 224)) {
        (Ok(detector), Ok(age)) => {
            tracing::info!("age-ai: models loaded ({MODEL_TAG})");
            Some(Models { detector, age })
        }
        (Err(e), _) | (_, Err(e)) => {
            tracing::warn!("age-ai: could not load models: {e} — pre-sorting disabled");
            None
        }
    }
}

fn models() -> Option<&'static Models> { MODELS.get_or_init(load).as_ref() }

/// True when the models are loaded and the feature is on.
pub fn available() -> bool { models().is_some() }

/// Warm the models at startup so the first submission does not pay the
/// load time (and so a broken model file shows up in the log right away).
pub async fn warm_up() {
    let _ = tokio::task::spawn_blocking(available).await;
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Estimate {
    /// 'likely_adult' | 'likely_minor' | 'uncertain' | 'no_face' | 'multi_face'
    pub verdict: &'static str,
    pub faces: usize,
    pub face_score: f32,
    /// Arg-max age bucket of the largest face (empty when no face).
    pub bucket: String,
    pub p_adult: f32,
    pub p_child: f32,
    pub p_teen: f32,
}

#[derive(Clone, Copy, Debug)]
struct Face { x1: f32, y1: f32, x2: f32, y2: f32, score: f32 }

fn iou(a: &Face, b: &Face) -> f32 {
    let ix = (a.x2.min(b.x2) - a.x1.max(b.x1)).max(0.0);
    let iy = (a.y2.min(b.y2) - a.y1.max(b.y1)).max(0.0);
    let inter = ix * iy;
    let area = |f: &Face| (f.x2 - f.x1).max(0.0) * (f.y2 - f.y1).max(0.0);
    let union = area(a) + area(b) - inter;
    if union <= 0.0 { 0.0 } else { inter / union }
}

/// Greedy non-maximum suppression (IoU 0.3), highest score first.
fn nms(mut faces: Vec<Face>) -> Vec<Face> {
    faces.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    let mut keep: Vec<Face> = Vec::new();
    for f in faces {
        if keep.iter().all(|k| iou(k, &f) < 0.3) { keep.push(f); }
    }
    keep
}

/// Verdict from the bucket probabilities (shared with the unit tests).
fn verdict_for(p: &[f32; 8]) -> (&'static str, f32, f32, f32) {
    let p_child = p[0] + p[1] + p[2];
    let p_teen = p[3];
    let p_adult = p[4] + p[5] + p[6] + p[7];
    let v = if p_adult >= ADULT_MIN { "likely_adult" }
        else if p_child >= CHILD_MIN { "likely_minor" }
        else { "uncertain" };
    (v, p_adult, p_child, p_teen)
}

fn run_sync(m: &Models, bytes: &[u8]) -> Result<Estimate, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("decode: {e}"))?.to_rgb8();
    let (w, h) = (img.width() as f32, img.height() as f32);

    // 1. detect faces on a 320x240 RGB, (x-127)/128
    let small = image::imageops::resize(&img, 320, 240, image::imageops::FilterType::Triangle);
    let det_in: Tensor = tract_ndarray::Array4::from_shape_fn((1, 3, 240, 320), |(_, c, y, x)| {
        (small.get_pixel(x as u32, y as u32)[c] as f32 - 127.0) / 128.0
    }).into();
    let out = m.detector.run(tvec!(det_in.into())).map_err(|e| format!("detector: {e}"))?;
    // outputs: scores (1,N,2) and boxes (1,N,4); identify by last dim
    let (scores, boxes) = {
        let a = out[0].to_plain_array_view::<f32>().map_err(|e| e.to_string())?.into_owned();
        let b = out[1].to_plain_array_view::<f32>().map_err(|e| e.to_string())?.into_owned();
        if a.shape()[2] == 2 { (a, b) } else { (b, a) }
    };
    let n = scores.shape()[1];
    let mut cands = Vec::new();
    for i in 0..n {
        let s = scores[[0, i, 1]];
        if s >= FACE_SCORE_MIN {
            cands.push(Face {
                x1: boxes[[0, i, 0]].clamp(0.0, 1.0) * w, y1: boxes[[0, i, 1]].clamp(0.0, 1.0) * h,
                x2: boxes[[0, i, 2]].clamp(0.0, 1.0) * w, y2: boxes[[0, i, 3]].clamp(0.0, 1.0) * h,
                score: s,
            });
        }
    }
    let faces = nms(cands);
    if faces.is_empty() {
        return Ok(Estimate { verdict: "no_face", faces: 0, face_score: 0.0, bucket: String::new(), p_adult: 0.0, p_child: 0.0, p_teen: 0.0 });
    }
    // largest face wins; more than one face is flagged for the reviewer
    let best = faces.iter().max_by(|a, b| {
        ((a.x2 - a.x1) * (a.y2 - a.y1)).partial_cmp(&((b.x2 - b.x1) * (b.y2 - b.y1))).unwrap_or(std::cmp::Ordering::Equal)
    }).copied().unwrap();

    // 2. crop with 20 % margin, resize 224x224, BGR minus (104,117,123)
    let bw = best.x2 - best.x1; let bh = best.y2 - best.y1;
    let x1 = (best.x1 - 0.2 * bw).max(0.0); let y1 = (best.y1 - 0.2 * bh).max(0.0);
    let x2 = (best.x2 + 0.2 * bw).min(w); let y2 = (best.y2 + 0.2 * bh).min(h);
    let cw = ((x2 - x1) as u32).max(8); let ch = ((y2 - y1) as u32).max(8);
    let crop = image::imageops::crop_imm(&img, x1 as u32, y1 as u32, cw, ch).to_image();
    let face224 = image::imageops::resize(&crop, 224, 224, image::imageops::FilterType::Triangle);
    let mean = [104.0f32, 117.0, 123.0]; // BGR
    let age_in: Tensor = tract_ndarray::Array4::from_shape_fn((1, 3, 224, 224), |(_, c, y, x)| {
        let px = face224.get_pixel(x as u32, y as u32);
        let v = px[2 - c] as f32; // RGB -> BGR
        v - mean[c]
    }).into();
    let out = m.age.run(tvec!(age_in.into())).map_err(|e| format!("age model: {e}"))?;
    let probs = out[0].to_plain_array_view::<f32>().map_err(|e| e.to_string())?;
    let mut p = [0f32; 8];
    for (i, v) in probs.iter().take(8).enumerate() { p[i] = *v; }
    let sum: f32 = p.iter().sum();
    if !(0.98..=1.02).contains(&sum) {
        // not a softmax output — normalise defensively
        let mx = p.iter().cloned().fold(f32::MIN, f32::max);
        let ex: Vec<f32> = p.iter().map(|v| (v - mx).exp()).collect();
        let s: f32 = ex.iter().sum();
        for i in 0..8 { p[i] = ex[i] / s; }
    }
    let (verdict, p_adult, p_child, p_teen) = verdict_for(&p);
    let verdict = if faces.len() > 1 { "multi_face" } else { verdict };
    let bucket = (0..8).max_by(|&a, &b| p[a].partial_cmp(&p[b]).unwrap()).map(|i| BUCKETS[i].to_string()).unwrap_or_default();
    Ok(Estimate { verdict, faces: faces.len(), face_score: best.score, bucket, p_adult, p_child, p_teen })
}

/// Estimate on a blocking thread. `Err` when the feature is unavailable or
/// the image cannot be processed; the caller records the error text.
pub async fn estimate(bytes: Vec<u8>) -> Result<Estimate, String> {
    tokio::task::spawn_blocking(move || {
        let m = models().ok_or_else(|| "age-ai unavailable".to_string())?;
        run_sync(m, &bytes)
    }).await.map_err(|e| format!("join: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_thresholds() {
        assert_eq!(verdict_for(&[0.0, 0.0, 0.0, 0.05, 0.6, 0.3, 0.05, 0.0]).0, "likely_adult");
        assert_eq!(verdict_for(&[0.3, 0.3, 0.1, 0.2, 0.1, 0.0, 0.0, 0.0]).0, "likely_minor");
        // 15-20 straddles 18: a teen-heavy distribution is never "adult"
        assert_eq!(verdict_for(&[0.0, 0.0, 0.1, 0.5, 0.4, 0.0, 0.0, 0.0]).0, "uncertain");
        assert_eq!(verdict_for(&[0.0, 0.0, 0.0, 0.2, 0.8, 0.0, 0.0, 0.0]).0, "uncertain");
    }

    #[test]
    fn nms_merges_overlaps() {
        let a = Face { x1: 0.0, y1: 0.0, x2: 100.0, y2: 100.0, score: 0.9 };
        let b = Face { x1: 5.0, y1: 5.0, x2: 105.0, y2: 105.0, score: 0.8 };
        let c = Face { x1: 300.0, y1: 300.0, x2: 400.0, y2: 400.0, score: 0.75 };
        let k = nms(vec![b, a, c]);
        assert_eq!(k.len(), 2);
        assert_eq!(k[0].score, 0.9);
    }

    /// Runs only when AGE_AI_MODEL_DIR and AGE_AI_TEST_FACES are set.
    #[tokio::test]
    async fn sample_faces_if_models_present() {
        let Ok(dir) = std::env::var("AGE_AI_TEST_FACES") else { return; };
        if !available() { eprintln!("models not present, skipping"); return; }
        let mut entries: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let bytes = std::fs::read(e.path()).unwrap();
            let t = std::time::Instant::now();
            let est = estimate(bytes).await.unwrap();
            eprintln!("{:<22} {:>12} faces={} score={:.2} bucket={:<6} adult={:.2} child={:.2} teen={:.2} ({} ms)",
                e.file_name().to_string_lossy(), est.verdict, est.faces, est.face_score, est.bucket, est.p_adult, est.p_child, est.p_teen, t.elapsed().as_millis());
            assert!(est.faces >= 1, "expected a face in {:?}", e.file_name());
        }
    }
}
