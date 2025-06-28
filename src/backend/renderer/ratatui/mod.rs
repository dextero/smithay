pub struct RatatuiRenderer;

#[cfg(feature = "wayland_frontend")]
impl ImportMemWl for RatatuiRenderer {}

#[cfg(feature = "wayland_frontend")]
impl ImportDmaWl for RatatuiRenderer {}

impl ImportMem for RatatuiRenderer {}

impl ImportDma for RatatuiRenderer {}

impl ExportMem for RatatuiRenderer {}

impl Bind<EGLSurface> for RatatuiRenderer {}

impl Bind<Dmabuf> for RatatuiRenderer {}

impl Bind<GlesTexture> for RatatuiRenderer {}

impl Bind<GlesRenderbuffer> for RatatuiRenderer {}

impl Offscreen<GlesTexture> for RatatuiRenderer {}

impl Offscreen<GlesRenderbuffer> for RatatuiRenderer {}
