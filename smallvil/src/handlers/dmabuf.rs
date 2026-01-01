use smithay::wayland::dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier};
use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::delegate_dmabuf;

use crate::Smallvil;

impl DmabufHandler for Smallvil {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_imported(&mut self, _global: &DmabufGlobal, _dmabuf: Dmabuf, notifier: ImportNotifier) {
        let _ = notifier.successful::<Self>();
    }
}

delegate_dmabuf!(Smallvil);
