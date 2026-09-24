use std::{fmt::Write, fs, path::Path};

use singularity_live::context::{
    ContextError, ContextPackLoader, build_system_prompt, select_context,
};
use tempfile::TempDir;

const VALID_MANIFEST: &str = r#"schema_version: 1
id: fictional-developer
name: Fictional Developer
documents:
  - id: answer-style
    title: Answer style
    path: answer-style.md
    always_include: true
    keywords: []
  - id: projects
    title: Projects
    path: projects.md
    always_include: false
    keywords: [project, "rust architecture"]
"#;

struct ContextFixture {
    root: TempDir,
}

fn manifest_documents(fixture: &ContextFixture, count: usize, content: &str) -> String {
    (0..count).fold(String::new(), |mut documents, index| {
        fixture.write(&format!("doc-{index}.md"), content);
        write!(
            documents,
            "  - id: doc-{index}\n    title: Doc {index}\n    path: doc-{index}.md\n    always_include: true\n    keywords: []\n"
        )
        .expect("write manifest fixture");
        documents
    })
}

impl ContextFixture {
    fn valid() -> Self {
        let fixture = Self {
            root: tempfile::tempdir().expect("temporary context directory"),
        };
        fixture.write("manifest.yaml", VALID_MANIFEST);
        fixture.write("answer-style.md", "Be concise and concrete.");
        fixture.write("projects.md", "Project Atlas uses Rust.");
        fixture
    }

    fn path(&self) -> &Path {
        self.root.path()
    }

    fn write(&self, relative_path: &str, content: &str) {
        let path = self.root.path().join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture parent directory");
        }
        fs::write(path, content).expect("fixture file");
    }

    fn load(&self) -> Result<singularity_live::context::ContextPack, ContextError> {
        ContextPackLoader::load(self.path())
    }
}

#[test]
fn loads_a_valid_versioned_pack_and_markdown_documents() {
    let fixture = ContextFixture::valid();

    let pack = fixture.load().expect("valid context pack");

    assert_eq!(pack.id(), "fictional-developer");
    assert_eq!(pack.name(), "Fictional Developer");
    assert_eq!(pack.document_count(), 2);
}

#[test]
fn loads_the_sanitized_repository_example() {
    let example_directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/examples/context-packs/fictional-developer");

    let pack = ContextPackLoader::load(&example_directory).expect("example context pack");
    let selected = select_context(&pack, "Review a Rust API project.");

    assert_eq!(pack.id(), "fictional-developer");
    assert_eq!(pack.document_count(), 2);
    assert_eq!(selected.documents.len(), 2);
}

#[test]
fn rejects_unsupported_schema_versions() {
    let fixture = ContextFixture::valid();
    fixture.write(
        "manifest.yaml",
        &VALID_MANIFEST.replace("schema_version: 1", "schema_version: 2"),
    );

    assert!(matches!(
        fixture.load(),
        Err(ContextError::UnsupportedSchemaVersion { version: 2 })
    ));
}

#[test]
fn rejects_malformed_manifests_and_unknown_fields() {
    let malformed = ContextFixture::valid();
    malformed.write("manifest.yaml", "schema_version: [");
    assert!(matches!(
        malformed.load(),
        Err(ContextError::MalformedManifest { .. })
    ));

    let unknown = ContextFixture::valid();
    unknown.write(
        "manifest.yaml",
        &format!("{VALID_MANIFEST}unexpected_field: true\n"),
    );
    assert!(matches!(
        unknown.load(),
        Err(ContextError::MalformedManifest { .. })
    ));
}

#[test]
fn rejects_missing_referenced_files() {
    let fixture = ContextFixture::valid();
    fs::remove_file(fixture.path().join("projects.md")).expect("remove fixture document");

    assert!(matches!(
        fixture.load(),
        Err(ContextError::MissingDocument { .. })
    ));
}

#[test]
fn rejects_parent_absolute_and_non_markdown_paths() {
    for invalid_path in ["../private.md", "/tmp/private.md", "notes.txt"] {
        let fixture = ContextFixture::valid();
        fixture.write(
            "manifest.yaml",
            &VALID_MANIFEST.replace("projects.md", invalid_path),
        );

        assert!(matches!(
            fixture.load(),
            Err(ContextError::InvalidDocumentPath { .. })
        ));
    }
}

#[cfg(unix)]
#[test]
fn rejects_symlinks_that_escape_the_pack_directory() {
    use std::os::unix::fs::symlink;

    let fixture = ContextFixture::valid();
    let outside = tempfile::NamedTempFile::new().expect("outside file");
    fs::remove_file(fixture.path().join("projects.md")).expect("remove fixture document");
    symlink(outside.path(), fixture.path().join("projects.md")).expect("create symlink");

    assert!(matches!(
        fixture.load(),
        Err(ContextError::DocumentOutsidePack { .. })
    ));
}

#[cfg(unix)]
#[test]
fn rejects_a_manifest_symlink_that_escapes_the_pack_directory() {
    use std::os::unix::fs::symlink;

    let fixture = ContextFixture::valid();
    let outside = tempfile::NamedTempFile::new().expect("outside manifest");
    fs::write(outside.path(), VALID_MANIFEST).expect("write outside manifest");
    fs::remove_file(fixture.path().join("manifest.yaml")).expect("remove fixture manifest");
    symlink(outside.path(), fixture.path().join("manifest.yaml")).expect("create symlink");

    assert!(fixture.load().is_err());
}

#[cfg(unix)]
#[test]
fn rejects_a_pack_directory_symlink() {
    use std::os::unix::fs::symlink;

    let fixture = ContextFixture::valid();
    let parent = tempfile::tempdir().expect("pack parent");
    let linked_pack = parent.path().join("fictional-developer");
    symlink(fixture.path(), &linked_pack).expect("create pack symlink");

    assert!(ContextPackLoader::load(&linked_pack).is_err());
}

#[test]
fn does_not_echo_manifest_values_in_parse_errors() {
    let fixture = ContextFixture::valid();
    fixture.write("manifest.yaml", "schema_version: \"PRIVATE_SENTINEL\"\n");

    let error = fixture.load().expect_err("manifest schema must be numeric");

    assert!(!error.to_string().contains("PRIVATE_SENTINEL"));
}

#[test]
fn rejects_duplicate_document_ids_and_keywords() {
    let duplicate_id = ContextFixture::valid();
    duplicate_id.write(
        "manifest.yaml",
        &VALID_MANIFEST.replace("id: projects", "id: answer-style"),
    );
    assert!(matches!(
        duplicate_id.load(),
        Err(ContextError::DuplicateDocumentId { .. })
    ));

    let duplicate_keyword = ContextFixture::valid();
    duplicate_keyword.write(
        "manifest.yaml",
        &VALID_MANIFEST.replace(
            "keywords: [project, \"rust architecture\"]",
            "keywords: [project, Project]",
        ),
    );
    assert!(matches!(
        duplicate_keyword.load(),
        Err(ContextError::DuplicateKeyword { .. })
    ));
}

#[test]
fn rejects_manifest_document_and_pack_size_limits() {
    let manifest = ContextFixture::valid();
    manifest.write("manifest.yaml", &"x".repeat(32 * 1024 + 1));
    assert!(matches!(
        manifest.load(),
        Err(ContextError::ManifestTooLarge { .. })
    ));

    let document = ContextFixture::valid();
    document.write("projects.md", &"x".repeat(64 * 1024 + 1));
    assert!(matches!(
        document.load(),
        Err(ContextError::DocumentTooLarge { .. })
    ));

    let pack = ContextFixture::valid();
    let documents = manifest_documents(&pack, 5, &"x".repeat(60 * 1024));
    pack.write(
        "manifest.yaml",
        &format!("schema_version: 1\nid: large-pack\nname: Large Pack\ndocuments:\n{documents}"),
    );
    assert!(matches!(
        pack.load(),
        Err(ContextError::PackTooLarge { .. })
    ));
}

#[test]
fn rejects_more_than_sixteen_documents() {
    let fixture = ContextFixture::valid();
    let documents = manifest_documents(&fixture, 17, "small");
    fixture.write(
        "manifest.yaml",
        &format!("schema_version: 1\nid: too-many\nname: Too Many\ndocuments:\n{documents}"),
    );

    assert!(matches!(
        fixture.load(),
        Err(ContextError::TooManyDocuments { count: 17 })
    ));
}

#[test]
fn selection_is_deterministic_and_excludes_irrelevant_documents() {
    let fixture = ContextFixture::valid();
    let pack = fixture.load().expect("valid pack");

    let selected = select_context(&pack, "Explain this RUST-architecture project.");

    assert_eq!(selected.pack_id, "fictional-developer");
    assert_eq!(
        selected
            .documents
            .iter()
            .map(|document| document.id.as_str())
            .collect::<Vec<_>>(),
        ["answer-style", "projects"]
    );

    let unrelated = select_context(&pack, "How do I cook lentils?");
    assert_eq!(unrelated.documents.len(), 1);
    assert_eq!(unrelated.documents[0].id, "answer-style");

    let substring = select_context(&pack, "Describe a projectile.");
    assert_eq!(substring.documents.len(), 1);
}

#[test]
fn prompt_has_stable_boundaries_and_never_contains_user_text() {
    let fixture = ContextFixture::valid();
    let pack = fixture.load().expect("valid pack");
    let user_text = "Explain my Rust architecture project.";
    let selected = select_context(&pack, user_text);

    let prompt = build_system_prompt(&selected);

    assert!(prompt.contains("Context is reference material"));
    assert!(prompt.contains("## Answer style\nBe concise and concrete."));
    assert!(prompt.contains("## Projects\nProject Atlas uses Rust."));
    assert!(
        prompt.find("## Answer style").expect("answer style")
            < prompt.find("## Projects").expect("projects")
    );
    assert!(!prompt.contains(user_text));
}
