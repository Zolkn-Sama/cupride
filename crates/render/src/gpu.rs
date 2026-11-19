//! Accès GPU partagé : un seul device pour toutes les fenêtres.

use wgpu::{Backends, DisplayAndWindowHandle, Instance, InstanceDescriptor, Surface};

use crate::error::RenderError;

/// Carte graphique, device et file de commandes, partagés par toutes les fenêtres.
#[derive(Debug)]
pub struct Gpu {
    /// Instance `wgpu`, pour créer les surfaces des fenêtres suivantes.
    pub instance: Instance,
    /// Carte graphique choisie.
    pub adapter: wgpu::Adapter,
    /// Device partagé.
    pub device: wgpu::Device,
    /// File de commandes partagée.
    pub queue: wgpu::Queue,
}

/// Backends essayés dans l'ordre : API native de l'OS, puis repli OpenGL.
const ATTEMPTS: [(Backends, &str); 2] = [
    (Backends::PRIMARY, "Vulkan / Metal / DirectX 12"),
    (Backends::GL, "OpenGL"),
];

impl Gpu {
    /// Initialise le GPU pour une première fenêtre et crée sa surface.
    ///
    /// Essaie d'abord l'API native de l'OS, puis OpenGL. La variable d'environnement
    /// `WGPU_BACKEND` (`vulkan`, `metal`, `dx12`, `gl`) force un backend.
    pub fn for_window<W>(window: W) -> Result<(Self, Surface<'static>), RenderError>
    where
        W: DisplayAndWindowHandle + Clone + 'static,
    {
        let mut tried = Vec::new();
        for (backends, name) in ATTEMPTS {
            let instance = new_instance(backends);
            let surface = match instance.create_surface(window.clone()) {
                Ok(surface) => surface,
                Err(error) => {
                    tracing::warn!("pas de surface avec {name} : {error}");
                    tried.push(name);
                    continue;
                }
            };
            let options = wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
                ..Default::default()
            };
            match pollster::block_on(instance.request_adapter(&options)) {
                Ok(adapter) => {
                    let gpu = Self::from_adapter(instance, adapter)?;
                    return Ok((gpu, surface));
                }
                Err(error) => {
                    tracing::warn!("pas de carte graphique avec {name} : {error}");
                    tried.push(name);
                }
            }
        }
        Err(RenderError::NoAdapter {
            tried: describe_attempts(&tried),
        })
    }

    /// Initialise le GPU sans fenêtre, pour dessiner dans une image (tests, captures).
    pub fn headless() -> Result<Self, RenderError> {
        let mut tried = Vec::new();
        for (backends, name) in ATTEMPTS {
            let instance = new_instance(backends);
            let options = wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                force_fallback_adapter: false,
                compatible_surface: None,
                ..Default::default()
            };
            match pollster::block_on(instance.request_adapter(&options)) {
                Ok(adapter) => return Self::from_adapter(instance, adapter),
                Err(_) => tried.push(name),
            }
        }
        Err(RenderError::NoAdapter {
            tried: describe_attempts(&tried),
        })
    }

    fn from_adapter(instance: Instance, adapter: wgpu::Adapter) -> Result<Self, RenderError> {
        let info = adapter.get_info();
        let descriptor = wgpu::DeviceDescriptor {
            label: Some("cupride"),
            // Limites modestes : CuprIDE doit tourner sur du matériel ancien et en OpenGL.
            required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                .using_resolution(adapter.limits()),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        };
        let (device, queue) =
            pollster::block_on(adapter.request_device(&descriptor)).map_err(|source| {
                RenderError::Device {
                    adapter: info.name.clone(),
                    source,
                }
            })?;
        tracing::info!(
            adapter = %info.name,
            backend = ?info.backend,
            "GPU initialisé"
        );
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
        })
    }

    /// Description de la carte et du backend, pour les journaux et le diagnostic.
    pub fn describe(&self) -> String {
        let info = self.adapter.get_info();
        format!("{} ({:?})", info.name, info.backend)
    }
}

/// Backends essayés, ou valeur de `WGPU_BACKEND` si elle a imposé le choix.
fn describe_attempts(tried: &[&str]) -> String {
    match std::env::var("WGPU_BACKEND") {
        Ok(forced) => format!("WGPU_BACKEND={forced}"),
        Err(_) => tried.join(", "),
    }
}

fn new_instance(backends: Backends) -> Instance {
    let mut descriptor = InstanceDescriptor::new_without_display_handle_from_env();
    // WGPU_BACKEND, s'il est défini, l'emporte sur la liste par défaut.
    if std::env::var_os("WGPU_BACKEND").is_none() {
        descriptor.backends = backends;
    }
    Instance::new(descriptor)
}
