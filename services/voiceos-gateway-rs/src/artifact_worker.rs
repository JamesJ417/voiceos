use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use voiceos_core::{ArtifactRecord, ConversationStore};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PdfSection {
    pub(crate) heading: String,
    pub(crate) body: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PdfSpec {
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) subtitle: String,
    #[serde(default)]
    pub(crate) sections: Vec<PdfSection>,
}

#[derive(Clone)]
pub(crate) struct PdfWorker {
    sender: mpsc::UnboundedSender<PdfJob>,
}

#[derive(Clone)]
pub(crate) struct ArtifactStorage {
    root: Arc<PathBuf>,
}

struct PdfJob {
    owner_id: String,
    artifact_id: String,
    task_id: Option<String>,
    description: String,
    spec: PdfSpec,
}

impl PdfWorker {
    pub(crate) fn start(store: Arc<ConversationStore>, storage: ArtifactStorage) -> Self {
        let (sender, mut receiver) = mpsc::unbounded_channel::<PdfJob>();
        tokio::spawn(async move {
            while let Some(job) = receiver.recv().await {
                let job_store = store.clone();
                let job_storage = storage.clone();
                tokio::task::spawn_blocking(move || run_job(job_store, job_storage, job))
                    .await
                    .ok();
            }
        });
        Self { sender }
    }

    pub(crate) fn enqueue(
        &self,
        owner_id: String,
        artifact_id: String,
        task_id: Option<String>,
        description: String,
        spec: PdfSpec,
    ) -> Result<(), &'static str> {
        self.sender
            .send(PdfJob {
                owner_id,
                artifact_id,
                task_id,
                description,
                spec,
            })
            .map_err(|_| "pdf_worker_unavailable")
    }
}

impl ArtifactStorage {
    pub(crate) fn new(root: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(&root)?;
        Ok(Self {
            root: Arc::new(root),
        })
    }

    fn write(&self, artifact_id: &str, bytes: &[u8]) -> std::io::Result<(String, String, u64)> {
        let shard = &artifact_id[..artifact_id.len().min(2)];
        let relative = PathBuf::from(shard).join(format!("{artifact_id}.pdf"));
        let target = self.root.join(&relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = target.with_extension("pdf.part");
        let mut file = fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, &target)?;
        let checksum = format!("{:x}", Sha256::digest(bytes));
        Ok((
            relative.to_string_lossy().replace('\\', "/"),
            checksum,
            bytes.len() as u64,
        ))
    }

    pub(crate) fn read_validated(&self, artifact: &ArtifactRecord) -> Result<Vec<u8>, String> {
        let key = artifact
            .storage_key
            .as_deref()
            .ok_or("artifact_storage_missing")?;
        let relative = Path::new(key);
        if relative.is_absolute()
            || relative
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err("artifact_storage_key_invalid".to_owned());
        }
        let bytes =
            fs::read(self.root.join(relative)).map_err(|_| "artifact_file_missing".to_owned())?;
        let actual = format!("{:x}", Sha256::digest(&bytes));
        if artifact.sha256.as_deref() != Some(actual.as_str())
            || artifact.byte_size != Some(bytes.len() as u64)
        {
            return Err("artifact_checksum_mismatch".to_owned());
        }
        Ok(bytes)
    }
}

fn run_job(store: Arc<ConversationStore>, storage: ArtifactStorage, job: PdfJob) {
    let result = (|| -> Result<(), String> {
        store
            .update_artifact_progress(&job.owner_id, &job.artifact_id, "generating", 20)
            .map_err(|e| e.to_string())?;
        let bytes = render_pdf(&job.spec)?;
        store
            .update_artifact_progress(&job.owner_id, &job.artifact_id, "validating", 80)
            .map_err(|e| e.to_string())?;
        let (key, checksum, size) = storage
            .write(&job.artifact_id, &bytes)
            .map_err(|e| e.to_string())?;
        let completed = store
            .complete_artifact(&job.owner_id, &job.artifact_id, &key, &checksum, size)
            .map_err(|e| e.to_string())?;
        storage.read_validated(&completed)?;
        if let Some(task_id) = &job.task_id {
            store
                .attach_task_artifact(
                    &job.owner_id,
                    task_id,
                    "pdf",
                    &completed.uri,
                    &job.description,
                    "vic",
                    "pdf-worker",
                )
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = store.fail_artifact(&job.owner_id, &job.artifact_id, &error);
    }
}

fn render_pdf(spec: &PdfSpec) -> Result<Vec<u8>, String> {
    if spec.title.trim().is_empty() {
        return Err("pdf_title_required".to_owned());
    }
    let mut content = String::new();
    content.push_str("0.98 0.99 0.99 rg 0 0 612 792 re f\n");
    content.push_str("0.208 0.878 0.757 rg 0 676 612 116 re f\n");
    pdf_text(&mut content, 42.0, 735.0, 24.0, true, &spec.title);
    if !spec.subtitle.trim().is_empty() {
        pdf_text(&mut content, 42.0, 707.0, 11.0, false, &spec.subtitle);
    }
    let mut y = 640.0;
    for section in &spec.sections {
        if y < 110.0 {
            break;
        }
        content.push_str(&format!("0.94 0.97 0.97 rg 36 {} 540 34 re f\n", y - 7.0));
        pdf_text(&mut content, 50.0, y + 4.0, 14.0, true, &section.heading);
        y -= 30.0;
        for line in wrap_text(&section.body, 82) {
            if y < 74.0 {
                break;
            }
            pdf_text(&mut content, 50.0, y, 10.5, false, &line);
            y -= 15.0;
        }
        y -= 13.0;
    }
    pdf_text(
        &mut content,
        42.0,
        34.0,
        8.5,
        false,
        "Created by VIC - VoiceOS artifact catalog",
    );
    build_pdf(&content)
}

fn pdf_text(output: &mut String, x: f32, y: f32, size: f32, bold: bool, text: &str) {
    let font = if bold { "F2" } else { "F1" };
    let safe = text
        .chars()
        .map(|c| if c.is_ascii() { c } else { '?' })
        .collect::<String>()
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)");
    output.push_str(&format!(
        "0.04 0.08 0.09 rg BT /{font} {size} Tf {x} {y} Td ({safe}) Tj ET\n"
    ));
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            if !line.is_empty() && line.len() + word.len() + 1 > width {
                lines.push(line);
                line = String::new();
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        if !line.is_empty() {
            lines.push(line);
        }
        if paragraph.is_empty() {
            lines.push(String::new());
        }
    }
    lines
}

fn build_pdf(content: &str) -> Result<Vec<u8>, String> {
    let stream = content.as_bytes();
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R /F2 5 0 R >> >> /Contents 6 0 R >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{}endstream", stream.len(), content),
    ];
    let mut pdf = b"%PDF-1.4\n%VoiceOS\n".to_vec();
    let mut offsets = vec![0usize];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, object).as_bytes());
    }
    let xref = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets.iter().skip(1) {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    if !pdf.starts_with(b"%PDF-") {
        return Err("pdf_render_failed".to_owned());
    }
    Ok(pdf)
}

pub(crate) fn recipe_card_spec() -> PdfSpec {
    PdfSpec {
        title: "VIC's Weeknight Herb Chicken".to_owned(),
        subtitle: "Recipe card - serves 4 - prep 15 min - cook 25 min".to_owned(),
        sections: vec![
            PdfSection { heading: "Ingredients".to_owned(), body: "4 boneless chicken breasts\n2 tablespoons olive oil\n1 teaspoon garlic powder\n1 teaspoon dried thyme\n1/2 teaspoon salt\n1/4 teaspoon black pepper\n1 lemon".to_owned() },
            PdfSection { heading: "Directions".to_owned(), body: "1. Heat the oven to 425 F. 2. Pat chicken dry and coat with olive oil. 3. Mix seasonings and rub over both sides. 4. Bake 20 to 25 minutes, until the center reaches 165 F. 5. Rest 5 minutes and finish with lemon.".to_owned() },
            PdfSection { heading: "VIC prep note".to_owned(), body: "Print on heavy paper, trim at the crop edge, and laminate after the ink is fully dry.".to_owned() },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipe_card_renderer_emits_a_complete_pdf() {
        let bytes = render_pdf(&recipe_card_spec()).unwrap();
        assert!(bytes.starts_with(b"%PDF-1.4"));
        assert!(bytes.ends_with(b"%%EOF\n"));
        assert!(
            bytes
                .windows(b"Ingredients".len())
                .any(|window| window == b"Ingredients")
        );
    }
}
