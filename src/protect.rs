//! Writing a copy of the PDF with a password, or without one. The document open in the viewer is
//! never touched: both only ever write a new file.
use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pdfcraft_cos::{Algorithm, Auth, CosError, Document, NewEncryption, ObjRef, Object, SaveOptions, write_full};

use crate::store::{self, Sidecar, Store};

/// What the notice says when the owner password is not the one.
pub const WRONG_OWNER: &str = "wrong owner password";

/// Every permission allowed: all of the `/P` bits set, as PDF writers do (`0xFFFFFFFC`).
const ALL_PERMISSIONS: i32 = -4;

/// What is done to the copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Job {
    Protect,
    Unprotect,
}

impl Job {
    /// The command that does it.
    pub fn command(self) -> &'static str {
        match self {
            Self::Protect => "protect",
            Self::Unprotect => "unprotect",
        }
    }

    fn suffix(self) -> &'static str {
        match self {
            Self::Protect => "-protected",
            Self::Unprotect => "-unprotected",
        }
    }
}

/// The copy's name when none is given: beside the document, with `-protected` or `-unprotected` added.
pub fn default_path(document: &Path, job: Job) -> PathBuf {
    let stem = document.file_stem().map_or_else(|| "document".to_owned(), |s| s.to_string_lossy().into_owned());
    document.with_file_name(format!("{stem}{}.pdf", job.suffix()))
}

/// How a document is protected, as far as the password it was opened with goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Security {
    /// Not password protected.
    None,
    /// Opened as the owner: the password may be changed or removed.
    Owner,
    /// Opened with a user password alone: the owner password is needed to change anything.
    UserOnly,
}

/// Runs `work` on a document that came from a stranger, a panic being an error.
fn guarded<T>(work: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(work)).map_err(|_| "the document could not be read".to_owned())?
}

fn open(bytes: Arc<Vec<u8>>, password: Option<&str>) -> Result<Document, String> {
    Document::open_with_password(bytes, password).map_err(|e| format!("{e}"))
}

pub fn security(bytes: Arc<Vec<u8>>, password: Option<&str>) -> Result<Security, String> {
    guarded(|| {
        let doc = open(bytes, password)?;
        Ok(match doc.security().map(|s| s.auth()) {
            None => Security::None,
            Some(Auth::Owner) => Security::Owner,
            Some(Auth::User) => Security::UserOnly,
        })
    })
}

/// A document open as its owner may have its password changed: the one opened with the user
/// password must be given the owner password.
fn owned(bytes: Arc<Vec<u8>>, password: Option<&str>, owner: Option<&str>) -> Result<Document, String> {
    let mut doc = open(bytes, password)?;
    if doc.security().is_some_and(|s| s.auth() == Auth::User) {
        let owner = owner.ok_or_else(|| "the owner password is needed".to_owned())?;
        doc.unlock(owner).map_err(|e| if matches!(e, CosError::WrongPassword) { WRONG_OWNER.to_owned() } else { format!("{e}") })?;
    }
    Ok(doc)
}

/// Whether `owner` is the owner password of the document, or not needed for it.
pub fn check_owner(bytes: Arc<Vec<u8>>, password: Option<&str>, owner: &str) -> Result<(), String> {
    guarded(|| owned(bytes, password, Some(owner)).map(drop))
}

/// The document with `new_password` as both its opening and its owner password, AES-256 and nothing
/// restricted. `seed` is the randomness of the keys.
pub fn protect(bytes: Arc<Vec<u8>>, password: Option<&str>, owner: Option<&str>, new_password: &str, seed: [u8; 32]) -> Result<Vec<u8>, String> {
    guarded(|| {
        let mut doc = owned(bytes, password, owner)?;
        let encryption = NewEncryption {
            algorithm: Algorithm::Aes256,
            user_password: new_password,
            owner_password: new_password,
            permissions: ALL_PERMISSIONS,
            encrypt_metadata: true,
            seed,
        };
        doc.set_encryption(&encryption).map_err(|e| format!("{e}"))?;
        write_full(&doc, &SaveOptions::default()).map_err(|e| format!("{e}"))
    })
}

/// The document without any password.
pub fn unprotect(bytes: Arc<Vec<u8>>, password: Option<&str>, owner: Option<&str>) -> Result<Vec<u8>, String> {
    guarded(|| {
        let mut doc = owned(bytes, password, owner)?;
        doc.remove_encryption();
        write_full(&doc, &SaveOptions::default()).map_err(|e| format!("{e}"))
    })
}

/// 32 bytes from the system, for the keys of a new password.
pub fn random_seed() -> Result<[u8; 32], String> {
    let mut seed = [0; 32];
    getrandom::fill(&mut seed).map_err(|e| format!("no randomness: {e}"))?;
    Ok(seed)
}

/// How deep and how wide the field tree is followed.
const MAX_DEPTH: usize = 64;
const MAX_FIELDS: usize = 100_000;

/// Whether the document has a signature field that is signed: of type `/Sig`, itself or by what it
/// inherits, with a value. A rewrite breaks such a signature.
pub fn is_signed(bytes: Arc<Vec<u8>>, password: Option<&str>) -> bool {
    guarded(|| Ok(open(bytes, password).is_ok_and(|doc| signed_fields(&doc)))).unwrap_or(false)
}

fn signed_fields(doc: &Document) -> bool {
    let Some(root) = doc.root() else { return false };
    let Some(acro) = doc.dict(&Object::Ref(root)).and_then(|c| c.get(b"AcroForm").and_then(|a| doc.dict(a))) else { return false };
    let Some(fields) = acro.get(b"Fields").map(|f| doc.resolve(f)) else { return false };
    let Some(fields) = fields.as_array() else { return false };
    // Each field with the type and the value it inherits.
    let mut pending: Vec<(Object, bool, bool, usize)> = fields.iter().map(|f| (f.clone(), false, false, 0)).collect();
    let mut seen: HashSet<ObjRef> = HashSet::new();
    let mut visited = 0;
    while let Some((object, sig, valued, depth)) = pending.pop() {
        visited += 1;
        if visited > MAX_FIELDS || depth > MAX_DEPTH {
            continue;
        }
        if let Some(reference) = object.as_ref()
            && !seen.insert(reference)
        {
            continue;
        }
        let Some(field) = doc.dict(&object) else { continue };
        let sig = field.get(b"FT").map_or(sig, |t| doc.resolve(t).as_name() == Some(b"Sig"));
        let valued = field.get(b"V").map_or(valued, |v| !matches!(*doc.resolve(v), Object::Null));
        if sig && valued {
            return true;
        }
        if let Some(kids) = field.get(b"Kids").map(|k| doc.resolve(k))
            && let Some(kids) = kids.as_array()
        {
            pending.extend(kids.iter().map(|k| (k.clone(), sig, valued, depth + 1)));
        }
    }
    false
}

/// Keeps what is saved for the open document (`side`) for the copy as well, under the copy's
/// content key and with the copy's path, so that the copy opens with the same annotations and form
/// values. Nothing is written for a locked document.
pub fn copy_sidecar(locked: bool, store: &Store, copy: &[u8], path: &Path, side: &Sidecar) -> Result<(), String> {
    if locked {
        return Ok(());
    }
    let store = store.rekeyed(store::content_key(copy));
    // Identical bytes from an earlier copy keep what was made of it since.
    if store.exists() {
        return Ok(());
    }
    store.save(&Sidecar { path: path.display().to_string(), ..side.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{Doc, OpenFailure, placeholder_pdf};

    fn plain() -> Arc<Vec<u8>> {
        Arc::new(placeholder_pdf())
    }

    /// The placeholder, protected with separate passwords.
    fn with_passwords(user: &str, owner: &str) -> Arc<Vec<u8>> {
        let mut doc = Document::open(plain()).unwrap();
        let encryption = NewEncryption {
            algorithm: Algorithm::Aes128,
            user_password: user,
            owner_password: owner,
            permissions: ALL_PERMISSIONS,
            encrypt_metadata: true,
            seed: [4; 32],
        };
        doc.set_encryption(&encryption).unwrap();
        Arc::new(write_full(&doc, &SaveOptions::default()).unwrap())
    }

    fn opens(bytes: &[u8], password: Option<&str>) -> Result<(), OpenFailure> {
        let config = crate::config::Config::load_str("").unwrap();
        Doc::from_bytes(Path::new("/x/a.pdf"), Arc::new(bytes.to_vec()), password, config.active_theme(), 8.0).map(drop)
    }

    #[test]
    fn a_protected_copy_needs_its_password() {
        let copy = protect(plain(), None, None, "secret", [1; 32]).unwrap();
        assert_eq!(opens(&copy, None), Err(OpenFailure::NeedsPassword));
        assert_eq!(opens(&copy, Some("wrong")), Err(OpenFailure::WrongPassword));
        assert_eq!(opens(&copy, Some("secret")), Ok(()));
        assert_eq!(security(Arc::new(copy), Some("secret")), Ok(Security::Owner));
    }

    #[test]
    fn the_password_of_a_protected_document_is_changed() {
        let bytes = with_passwords("same", "same");
        let copy = protect(bytes, Some("same"), None, "new", [2; 32]).unwrap();
        assert_eq!(opens(&copy, Some("same")), Err(OpenFailure::WrongPassword));
        assert_eq!(opens(&copy, Some("new")), Ok(()));
    }

    #[test]
    fn removing_the_password_leaves_a_document_that_opens_without_one() {
        let bytes = with_passwords("same", "same");
        assert_eq!(security(bytes.clone(), Some("same")), Ok(Security::Owner));
        let copy = unprotect(bytes, Some("same"), None).unwrap();
        assert_eq!(opens(&copy, None), Ok(()));
        assert_eq!(security(Arc::new(copy), None), Ok(Security::None));
        assert_eq!(security(plain(), None), Ok(Security::None));
    }

    #[test]
    fn a_user_only_password_needs_the_owner_password_to_change_anything() {
        let bytes = with_passwords("user", "owner");
        assert_eq!(security(bytes.clone(), Some("user")), Ok(Security::UserOnly));
        assert_eq!(security(bytes.clone(), Some("owner")), Ok(Security::Owner));
        assert_eq!(check_owner(bytes.clone(), Some("user"), "nope"), Err(WRONG_OWNER.to_owned()));
        assert_eq!(check_owner(bytes.clone(), Some("user"), "owner"), Ok(()));
        assert!(protect(bytes.clone(), Some("user"), None, "new", [3; 32]).is_err());
        assert_eq!(protect(bytes.clone(), Some("user"), Some("nope"), "new", [3; 32]), Err(WRONG_OWNER.to_owned()));
        assert!(unprotect(bytes.clone(), Some("user"), None).is_err());
        assert_eq!(unprotect(bytes.clone(), Some("user"), Some("nope")), Err(WRONG_OWNER.to_owned()));
        let copy = protect(bytes.clone(), Some("user"), Some("owner"), "new", [3; 32]).unwrap();
        assert_eq!(opens(&copy, Some("user")), Err(OpenFailure::WrongPassword));
        assert_eq!(opens(&copy, Some("new")), Ok(()));
        let open = unprotect(bytes, Some("user"), Some("owner")).unwrap();
        assert_eq!(opens(&open, None), Ok(()));
    }

    #[test]
    fn the_copy_is_named_beside_the_document() {
        assert_eq!(default_path(Path::new("/a/form.pdf"), Job::Protect), Path::new("/a/form-protected.pdf"));
        assert_eq!(default_path(Path::new("/a/form.pdf"), Job::Unprotect), Path::new("/a/form-unprotected.pdf"));
    }

    #[test]
    fn the_document_is_never_replaced_and_a_file_only_when_forced() {
        use crate::export::{refusal, write_atomic};
        let dir = std::env::temp_dir().join(format!("yavcpdf-protect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (original, copy) = (dir.join("a.pdf"), dir.join("a-protected.pdf"));
        std::fs::write(&original, b"a").unwrap();
        assert!(refusal(&original, &original, true, "protect").unwrap().starts_with("protect writes a copy"));
        assert_eq!(refusal(&original, &copy, false, "protect"), None);
        write_atomic(&copy, b"b", false, None).unwrap();
        assert_eq!(refusal(&original, &copy, false, "protect"), Some(format!("{} exists: use :protect! to replace it", copy.display())));
        assert!(refusal(&original, &copy, false, "unprotect").unwrap().ends_with("use :unprotect! to replace it"));
        assert_eq!(refusal(&original, &copy, true, "protect"), None);
        assert!(write_atomic(&copy, b"c", false, None).is_err());
        assert_eq!(std::fs::read(&copy).unwrap(), b"b");
        write_atomic(&copy, b"c", true, None).unwrap();
        assert_eq!(std::fs::read(&copy).unwrap(), b"c");
        assert_eq!(std::fs::read(&original).unwrap(), b"a");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2, "no temporary file is left");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn form_pdf(objects: &[&str]) -> Arc<Vec<u8>> {
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
        for o in offsets {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
        Arc::new(out)
    }

    fn form_with(fields: &str, rest: &[&str]) -> Arc<Vec<u8>> {
        let acro = format!("<< /Fields {fields} >>");
        let mut objects = vec![
            "<< /Type /Catalog /Pages 2 0 R /AcroForm 4 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >>",
            "<< /Type /Page /Parent 2 0 R >>",
            &acro,
        ];
        objects.extend_from_slice(rest);
        form_pdf(&objects)
    }

    #[test]
    fn a_signed_signature_field_is_found() {
        let sig = "<< /Type /Annot /Subtype /Widget /FT /Sig /T (s) /V 6 0 R /Rect [0 0 0 0] /P 3 0 R >>";
        let value = "<< /Type /Sig /Filter /Adobe.PPKLite /Contents <00> >>";
        assert!(is_signed(form_with("[5 0 R]", &[sig, value]), None));
        // Empty, or of another type.
        let empty = "<< /Type /Annot /Subtype /Widget /FT /Sig /T (s) /Rect [0 0 0 0] /P 3 0 R >>";
        assert!(!is_signed(form_with("[5 0 R]", &[empty]), None));
        let text = "<< /Type /Annot /Subtype /Widget /FT /Tx /T (s) /V (x) /Rect [0 0 0 0] /P 3 0 R >>";
        assert!(!is_signed(form_with("[5 0 R]", &[text]), None));
        // The type and the value from the parent.
        let parent = "<< /FT /Sig /V 7 0 R /T (p) /Kids [6 0 R] >>";
        let kid = "<< /Type /Annot /Subtype /Widget /Parent 5 0 R /Rect [0 0 0 0] /P 3 0 R >>";
        assert!(is_signed(form_with("[5 0 R]", &[parent, kid, value]), None));
        // A loop of kids ends.
        let a = "<< /T (a) /Kids [6 0 R] >>";
        let b = "<< /T (b) /Kids [5 0 R] >>";
        assert!(!is_signed(form_with("[5 0 R]", &[a, b]), None));
        assert!(!is_signed(plain(), None));
        assert!(!is_signed(Arc::new(b"nonsense".to_vec()), None));
    }

    #[test]
    fn the_sidecar_of_the_copy_has_its_key_and_its_path() {
        let dir = std::env::temp_dir().join(format!("yavcpdf-protect-side-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::new(dir.clone(), "original".to_owned());
        let side = Sidecar { path: "/a/a.pdf".into(), zoom: Some(1.5), ..Default::default() };
        let copy = protect(plain(), None, None, "pw", [7; 32]).unwrap();
        let target = Path::new("/a/a-protected.pdf");
        copy_sidecar(true, &store, &copy, target, &side).unwrap();
        assert!(!dir.exists(), "a locked document writes nothing");
        copy_sidecar(false, &store, &copy, target, &side).unwrap();
        let file = dir.join(format!("{}.json", store::content_key(&copy)));
        let saved = std::fs::read_to_string(file).unwrap();
        assert!(saved.contains("/a/a-protected.pdf") && !saved.contains("/a/a.pdf"), "{saved}");
        assert!(saved.contains("1.5"), "{saved}");
        assert!(!dir.join("original.json").exists());
        assert_eq!(side.path, "/a/a.pdf");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_copy_is_aes_256_revision_6_with_every_permission() {
        let copy = Arc::new(protect(plain(), None, None, "secret", [1; 32]).unwrap());
        let doc = Document::open_with_password(copy.clone(), Some("secret")).unwrap();
        let encrypt = doc.trailer().get(b"Encrypt").and_then(|e| doc.dict(e)).expect("an /Encrypt dictionary");
        assert_eq!(encrypt.int(b"V"), Some(5));
        assert_eq!(encrypt.int(b"R"), Some(6));
        assert_eq!(encrypt.int(b"Length"), Some(256));
        let text = String::from_utf8_lossy(&copy);
        assert!(text.contains("AESV3"), "AES-256 crypt filter");
        let permissions = doc.permissions().unwrap();
        assert_eq!(permissions.revision, 6);
        assert_eq!(permissions.bits, ALL_PERMISSIONS);
        assert!(permissions.owner);
        // The user password opens it just as well, and the document is unchanged in content.
        assert_eq!(security(copy, Some("secret")), Ok(Security::Owner));
    }

    #[test]
    fn a_document_opened_as_its_owner_is_protected_without_asking() {
        let bytes = with_passwords("user", "owner");
        let copy = protect(bytes.clone(), Some("owner"), None, "new", [5; 32]).unwrap();
        assert_eq!(opens(&copy, Some("new")), Ok(()));
        assert_eq!(opens(&copy, Some("owner")), Err(OpenFailure::WrongPassword));
        let open = unprotect(bytes.clone(), Some("owner"), None).unwrap();
        assert_eq!(opens(&open, None), Ok(()));
        // A wrong owner is not consulted for a document already open as owner.
        assert!(protect(bytes, Some("owner"), Some("ignored"), "new", [5; 32]).is_ok());
    }

    #[test]
    fn a_wrong_document_password_is_an_error_and_unprotecting_a_plain_one_works() {
        let bytes = with_passwords("user", "owner");
        assert!(protect(bytes.clone(), Some("bad"), Some("owner"), "new", [5; 32]).is_err());
        assert!(protect(bytes, None, Some("owner"), "new", [5; 32]).is_err());
        let copy = unprotect(plain(), None, None).unwrap();
        assert_eq!(opens(&copy, None), Ok(()));
        assert_eq!(security(Arc::new(copy), None), Ok(Security::None));
    }

    #[test]
    fn a_signature_field_without_a_value_or_with_a_null_value_is_not_signed() {
        let null = "<< /Type /Annot /Subtype /Widget /FT /Sig /T (s) /V null /Rect [0 0 0 0] /P 3 0 R >>";
        assert!(!is_signed(form_with("[5 0 R]", &[null]), None));
        let inherited = "<< /FT /Sig /T (p) /Kids [6 0 R] >>";
        let kid = "<< /Type /Annot /Subtype /Widget /Parent 5 0 R /Rect [0 0 0 0] /P 3 0 R >>";
        assert!(!is_signed(form_with("[5 0 R]", &[inherited, kid]), None));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_to_the_document_is_the_document() {
        use crate::export::refusal;
        let dir = std::env::temp_dir().join(format!("yavcpdf-protect-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (original, link) = (dir.join("a.pdf"), dir.join("link.pdf"));
        std::fs::write(&original, b"a").unwrap();
        std::os::unix::fs::symlink(&original, &link).unwrap();
        assert!(refusal(&original, &link, true, "protect").unwrap().starts_with("protect writes a copy"));
        assert!(refusal(&link, &original, true, "unprotect").unwrap().starts_with("unprotect writes a copy"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn write_atomic_leaves_no_temporary_file() {
        let dir = std::env::temp_dir().join(format!("yavcpdf-protect-tmp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let copy = dir.join("a-protected.pdf");
        crate::export::write_atomic(&copy, b"x", false, None).unwrap();
        assert!(copy.exists());
        assert!(crate::export::write_atomic(&dir.join("missing").join("b.pdf"), b"x", false, None).is_err());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_name_with_and_without_extension_do_not_share_a_temporary_file() {
        let dir = std::env::temp_dir().join(format!("yavcpdf-protect-names-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a"), dir.join("a.pdf"));
        crate::export::write_atomic(&a, b"1", false, None).unwrap();
        crate::export::write_atomic(&b, b"2", false, None).unwrap();
        // A temporary file in the way is never reused, and is left alone.
        let hidden = dir.join(format!(".c.{}.tmp", std::process::id()));
        std::fs::write(&hidden, b"mine").unwrap();
        assert!(crate::export::write_atomic(&dir.join("c"), b"3", true, None).is_err());
        assert_eq!(std::fs::read(&hidden).unwrap(), b"mine");
        assert!(!dir.join("c").exists());
        assert_eq!(std::fs::read(&a).unwrap(), b"1");
        assert_eq!(std::fs::read(&b).unwrap(), b"2");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_copy_has_the_given_mode_and_a_late_file_is_not_clobbered() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("yavcpdf-protect-mode-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let copy = dir.join("a-protected.pdf");
        crate::export::write_atomic(&copy, b"x", false, Some(0o600)).unwrap();
        assert_eq!(std::fs::metadata(&copy).unwrap().permissions().mode() & 0o777, 0o600);
        // The file is there now: the plain form refuses and leaves it and no temporary file.
        assert!(crate::export::write_atomic(&copy, b"y", false, Some(0o600)).is_err());
        assert_eq!(std::fs::read(&copy).unwrap(), b"x");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_existing_sidecar_of_the_copy_is_kept() {
        let dir = std::env::temp_dir().join(format!("yavcpdf-protect-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::new(dir.clone(), "original".to_owned());
        let copy = unprotect(plain(), None, None).unwrap();
        let target = Path::new("/a/a-unprotected.pdf");
        let first = Sidecar { path: "/a/a.pdf".into(), zoom: Some(1.5), ..Default::default() };
        copy_sidecar(false, &store, &copy, target, &first).unwrap();
        let later = Sidecar { path: "/a/a.pdf".into(), zoom: Some(2.5), ..Default::default() };
        copy_sidecar(false, &store, &copy, target, &later).unwrap();
        let saved = std::fs::read_to_string(dir.join(format!("{}.json", store::content_key(&copy)))).unwrap();
        assert!(saved.contains("1.5") && !saved.contains("2.5"), "{saved}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn export_refusal_messages_are_unchanged() {
        use crate::export::refusal;
        let dir = std::env::temp_dir().join(format!("yavcpdf-protect-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (original, copy) = (dir.join("a.pdf"), dir.join("b.pdf"));
        std::fs::write(&original, b"a").unwrap();
        std::fs::write(&copy, b"b").unwrap();
        assert_eq!(refusal(&original, &original, false, "export").as_deref(), Some("export writes a copy, not over the document itself"));
        assert_eq!(refusal(&original, &copy, false, "export"), Some(format!("{} exists: use :export! to replace it", copy.display())));
        assert_eq!(refusal(&original, &copy, true, "export"), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_seed_is_random() {
        assert_ne!(random_seed().unwrap(), random_seed().unwrap());
    }
}
