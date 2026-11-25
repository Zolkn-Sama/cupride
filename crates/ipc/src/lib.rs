//! Instance unique de CuprIDE.
//!
//! 1. Au lancement, `cupride` se connecte au socket local de l'utilisateur ([`connect`]).
//! 2. Si une instance répond, la ligne de commande lui est transmise ([`Request`]) ; elle répond
//!    par des messages ([`Reply`]), le dernier donnant le code de sortie, et le nouveau processus
//!    se termine.
//! 3. Sinon, le processus devient l'instance principale ([`Server`]) : un thread écoute le socket
//!    et transmet chaque demande ([`Incoming`]) au thread principal.
//!
//! # Sécurité
//!
//! - **Unix** : le socket est un fichier, dans un dossier privé (`0700`) qui appartient à
//!   l'utilisateur, dans `$XDG_RUNTIME_DIR`, sinon dans le dossier temporaire de l'utilisateur.
//!   Un dossier qui appartiendrait à quelqu'un d'autre, ou accessible à d'autres, est refusé.
//! - **Windows** : un tube nommé propre à l'utilisateur ; seul son créateur (et le système) peut
//!   y écrire, et les clients distants sont refusés.
//!
//! # Socket orphelin
//!
//! Après un plantage, le fichier du socket reste sur le disque sans personne pour l'écouter : la
//! connexion échoue, et la nouvelle instance le remplace. Le remplacement n'a lieu qu'après une
//! connexion refusée : une instance vivante n'est jamais délogée.
//!
//! Cette crate n'a aucune dépendance interne.

use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{ListenerOptions, Name, Stream};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Version du protocole : une instance refuse une demande d'une autre version.
pub const PROTOCOL_VERSION: u32 = 1;

/// Ligne de commande transmise à l'instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    /// Version du protocole.
    pub version: u32,
    /// Dossier courant du terminal : les chemins relatifs en partent.
    pub cwd: PathBuf,
    /// Arguments, sans le nom du programme.
    pub args: Vec<String>,
}

impl Request {
    /// Demande de la version courante du protocole.
    pub fn new(cwd: PathBuf, args: Vec<String>) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            cwd,
            args,
        }
    }
}

/// Message de l'instance au terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Reply {
    /// À afficher sur la sortie standard.
    Output {
        /// Texte.
        text: String,
    },
    /// À afficher sur la sortie d'erreur.
    Error {
        /// Texte.
        text: String,
    },
    /// Fin : le processus du terminal sort avec ce code.
    Exit {
        /// Code de sortie.
        code: i32,
    },
}

/// Erreur de l'instance unique.
#[derive(Debug, Error)]
pub enum IpcError {
    /// Erreur du système (socket, dossier).
    #[error("instance unique : {0}")]
    Io(#[from] io::Error),
    /// Le dossier du socket n'est pas sûr.
    #[error("instance unique : dossier {} non sûr ({reason}) : refusé", path.display())]
    Unsafe {
        /// Dossier concerné.
        path: PathBuf,
        /// Pourquoi.
        reason: &'static str,
    },
    /// Message illisible.
    #[error("instance unique : message illisible ({0})")]
    Format(#[from] serde_json::Error),
}

/// Adresse du socket de l'instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    #[cfg(unix)]
    path: PathBuf,
    #[cfg(not(unix))]
    name: String,
}

impl Endpoint {
    /// Socket de l'utilisateur courant. Sous Unix, son dossier privé est créé au besoin, et
    /// vérifié.
    pub fn for_user() -> Result<Self, IpcError> {
        #[cfg(unix)]
        {
            let base = std::env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute() && p.is_dir())
                .unwrap_or_else(std::env::temp_dir);
            let uid = unix::current_uid(&base)?;
            let dir = base.join(format!("cupride-{uid}"));
            unix::private_dir(&dir, uid)?;
            Ok(Self::at(dir.join("instance.sock")))
        }
        #[cfg(not(unix))]
        {
            let user = std::env::var("USERNAME").unwrap_or_else(|_| "utilisateur".to_owned());
            let user: String = user
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '_' })
                .collect();
            Ok(Self {
                name: format!("cupride-{user}.sock"),
            })
        }
    }

    /// Socket au chemin `path` (Unix) ; sous Windows, tube nommé d'après le nom du fichier.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        #[cfg(unix)]
        {
            Self { path }
        }
        #[cfg(not(unix))]
        {
            Self {
                name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "cupride.sock".to_owned()),
            }
        }
    }

    /// Emplacement, pour les journaux.
    pub fn describe(&self) -> String {
        #[cfg(unix)]
        {
            self.path.display().to_string()
        }
        #[cfg(not(unix))]
        {
            format!(r"\\.\pipe\{}", self.name)
        }
    }

    fn name(&self) -> io::Result<Name<'_>> {
        #[cfg(unix)]
        {
            use interprocess::local_socket::GenericFilePath;
            self.path.as_path().to_fs_name::<GenericFilePath>()
        }
        #[cfg(not(unix))]
        {
            use interprocess::local_socket::GenericNamespaced;
            self.name.as_str().to_ns_name::<GenericNamespaced>()
        }
    }

    /// Chemin du socket (Unix).
    #[cfg(unix)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(unix)]
mod unix {
    use std::fs;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    use std::path::Path;

    use super::IpcError;

    /// Identifiant de l'utilisateur courant : propriétaire d'un fichier qu'on vient de créer.
    pub(super) fn current_uid(dir: &Path) -> Result<u32, IpcError> {
        let probe = tempfile::tempfile_in(dir).or_else(|_| tempfile::tempfile())?;
        Ok(probe.metadata()?.uid())
    }

    /// Crée le dossier `dir` en `0700`, ou vérifie celui qui existe : un vrai dossier (pas un
    /// lien), à l'utilisateur `uid`, inaccessible aux autres.
    pub(super) fn private_dir(dir: &Path, uid: u32) -> Result<(), IpcError> {
        match fs::DirBuilder::new().mode(0o700).create(dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        let unsafe_dir = |reason| IpcError::Unsafe {
            path: dir.to_owned(),
            reason,
        };
        let meta = fs::symlink_metadata(dir)?;
        if !meta.is_dir() {
            return Err(unsafe_dir("ce n'est pas un dossier"));
        }
        if meta.uid() != uid {
            return Err(unsafe_dir("il appartient à un autre utilisateur"));
        }
        if meta.permissions().mode() & 0o077 != 0 {
            return Err(unsafe_dir("il est accessible à d'autres utilisateurs"));
        }
        Ok(())
    }
}

/// Connexion à une instance en marche.
#[derive(Debug)]
pub struct Client {
    stream: Stream,
}

/// Se connecte à l'instance de `endpoint`. `None` : aucune instance ne répond (pas de socket,
/// ou socket orphelin d'un plantage).
pub fn connect(endpoint: &Endpoint) -> Result<Option<Client>, IpcError> {
    match Stream::connect(endpoint.name()?) {
        Ok(stream) => Ok(Some(Client { stream })),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

impl Client {
    /// Envoie la demande, puis lit les réponses jusqu'à [`Reply::Exit`] ; `on_reply` reçoit chaque
    /// message. Renvoie le code de sortie. Une instance qui ferme la connexion sans conclure
    /// donne le code 1.
    pub fn send(
        mut self,
        request: &Request,
        mut on_reply: impl FnMut(&Reply),
    ) -> Result<i32, IpcError> {
        let mut line = serde_json::to_vec(request)?;
        line.push(b'\n');
        self.stream.write_all(&line)?;
        self.stream.flush()?;
        let reader = BufReader::new(self.stream);
        for line in reader.lines() {
            let reply: Reply = serde_json::from_str(&line?)?;
            on_reply(&reply);
            if let Reply::Exit { code } = reply {
                return Ok(code);
            }
        }
        Ok(1)
    }
}

/// Instance principale : écoute le socket.
#[derive(Debug)]
pub struct Server {
    listener: interprocess::local_socket::Listener,
    endpoint: Endpoint,
}

/// Demande reçue d'un terminal, et de quoi lui répondre.
#[derive(Debug)]
pub struct Incoming {
    /// Ligne de commande.
    pub request: Request,
    /// Pour répondre.
    pub responder: Responder,
}

/// Réponses à un terminal ; la connexion se ferme quand il est abandonné.
#[derive(Debug)]
pub struct Responder {
    stream: Stream,
}

impl Responder {
    /// Envoie un message. Un terminal fermé entre-temps n'est pas une erreur grave : elle est
    /// renvoyée, à journaliser.
    pub fn reply(&mut self, reply: &Reply) -> Result<(), IpcError> {
        let mut line = serde_json::to_vec(reply)?;
        line.push(b'\n');
        self.stream.write_all(&line)?;
        self.stream.flush()?;
        Ok(())
    }

    /// Envoie les messages et le code de sortie, puis ferme la connexion.
    pub fn finish(mut self, replies: &[Reply], code: i32) -> Result<(), IpcError> {
        for reply in replies {
            self.reply(reply)?;
        }
        self.reply(&Reply::Exit { code })
    }
}

impl Server {
    /// Devient l'instance principale sur `endpoint`. À n'appeler qu'après un [`connect`] sans
    /// réponse : un socket orphelin est alors remplacé.
    pub fn bind(endpoint: &Endpoint) -> Result<Self, IpcError> {
        let listener = ListenerOptions::new()
            .name(endpoint.name()?)
            .try_overwrite(true)
            .create_sync()?;
        Ok(Self {
            listener,
            endpoint: endpoint.clone(),
        })
    }

    /// Adresse écoutée.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Écoute dans un thread à part. Chaque connexion est lue dans son propre thread, et chaque
    /// demande valide est passée à `on_request` ; une demande illisible ou d'une autre version
    /// reçoit une erreur.
    pub fn spawn(
        self,
        on_request: impl Fn(Incoming) + Send + Sync + 'static,
    ) -> io::Result<JoinHandle<()>> {
        let on_request = std::sync::Arc::new(on_request);
        std::thread::Builder::new()
            .name("cupride-ipc".to_owned())
            .spawn(move || {
                for stream in self.listener.incoming() {
                    let stream = match stream {
                        Ok(stream) => stream,
                        Err(error) => {
                            tracing::warn!("instance unique : connexion refusée : {error}");
                            continue;
                        }
                    };
                    let on_request = std::sync::Arc::clone(&on_request);
                    let spawned = std::thread::Builder::new()
                        .name("cupride-ipc-client".to_owned())
                        .spawn(move || {
                            if let Some(incoming) = read_request(stream) {
                                on_request(incoming);
                            }
                        });
                    if let Err(error) = spawned {
                        tracing::warn!("instance unique : {error}");
                    }
                }
            })
    }
}

/// Lit la demande d'une connexion ; répond par une erreur si elle est illisible.
fn read_request(stream: Stream) -> Option<Incoming> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if let Err(error) = reader.read_line(&mut line) {
        tracing::debug!("instance unique : lecture impossible : {error}");
        return None;
    }
    let responder = Responder {
        stream: reader.into_inner(),
    };
    let problem = match serde_json::from_str::<Request>(&line) {
        Ok(request) if request.version == PROTOCOL_VERSION => {
            return Some(Incoming { request, responder });
        }
        Ok(request) => format!(
            "version du protocole {} au lieu de {PROTOCOL_VERSION} : relancer avec la même \
             version de CuprIDE",
            request.version
        ),
        Err(error) => format!("demande illisible : {error}"),
    };
    let _ = responder.finish(&[Reply::Error { text: problem }], 2);
    None
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::mpsc;

    fn endpoint(dir: &Path) -> Endpoint {
        Endpoint::at(dir.join("test.sock"))
    }

    #[test]
    fn aller_retour() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = endpoint(dir.path());
        assert!(connect(&endpoint).unwrap().is_none(), "aucune instance");
        let server = Server::bind(&endpoint).unwrap();
        let (sender, received) = mpsc::channel();
        server
            .spawn(move |incoming: Incoming| {
                let args = incoming.request.args.clone();
                sender.send(incoming.request.clone()).unwrap();
                incoming
                    .responder
                    .finish(
                        &[Reply::Output {
                            text: args.join("+"),
                        }],
                        3,
                    )
                    .unwrap();
            })
            .unwrap();
        let client = connect(&endpoint).unwrap().expect("instance trouvée");
        let request = Request::new("/tmp".into(), vec!["a.rs".into(), "b.rs".into()]);
        let mut replies = Vec::new();
        let code = client.send(&request, |r| replies.push(r.clone())).unwrap();
        assert_eq!(code, 3);
        assert_eq!(
            replies[0],
            Reply::Output {
                text: "a.rs+b.rs".into()
            }
        );
        assert_eq!(received.recv().unwrap(), request);
    }

    #[test]
    fn mauvaise_version_refusee() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = endpoint(dir.path());
        Server::bind(&endpoint)
            .unwrap()
            .spawn(|_| panic!("ne doit pas arriver"))
            .unwrap();
        let mut request = Request::new("/".into(), vec![]);
        request.version = 99;
        let mut errors = Vec::new();
        let code = connect(&endpoint)
            .unwrap()
            .unwrap()
            .send(&request, |r| errors.push(r.clone()))
            .unwrap();
        assert_eq!(code, 2);
        assert!(matches!(&errors[0], Reply::Error { text } if text.contains("version")));
    }

    #[test]
    fn socket_orphelin_remplace() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = endpoint(dir.path());
        // Un socket sans personne à l'écoute, comme après un plantage.
        let orphan = std::os::unix::net::UnixListener::bind(endpoint.path()).unwrap();
        drop(orphan);
        assert!(endpoint.path().exists());
        assert!(connect(&endpoint).unwrap().is_none(), "connexion refusée");
        Server::bind(&endpoint)
            .unwrap()
            .spawn(|incoming| incoming.responder.finish(&[], 0).unwrap())
            .unwrap();
        let client = connect(&endpoint).unwrap().expect("nouvelle instance");
        assert_eq!(
            client
                .send(&Request::new("/".into(), vec![]), |_| {})
                .unwrap(),
            0
        );
    }

    #[test]
    fn dossier_prive() {
        let dir = tempfile::tempdir().unwrap();
        let uid = unix::current_uid(dir.path()).unwrap();
        let private = dir.path().join("cupride");
        unix::private_dir(&private, uid).unwrap();
        let mode = std::fs::metadata(&private).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        unix::private_dir(&private, uid).unwrap();
        // Ouvert aux autres : refusé.
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(matches!(
            unix::private_dir(&private, uid),
            Err(IpcError::Unsafe { .. })
        ));
        // Appartenant à un autre utilisateur : refusé.
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(unix::private_dir(&private, uid + 1).is_err());
        // Un lien symbolique : refusé.
        let link = dir.path().join("lien");
        std::os::unix::fs::symlink(&private, &link).unwrap();
        assert!(unix::private_dir(&link, uid).is_err());
    }

    #[test]
    fn socket_de_l_utilisateur() {
        let endpoint = Endpoint::for_user().unwrap();
        assert!(endpoint.describe().ends_with("instance.sock"));
    }
}
