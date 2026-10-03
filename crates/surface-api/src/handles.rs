//! Handles
//!
//! Sealed resource values shared by every sealer.

use mlua_extras::TypedUserData;
use mlua_extras::typeduserdata_impl;

use confit_store::Stores;
use confit_store::handles::ResourceHandle as StoreResource;
use confit_store::handles::TrustedHandle;

/// Resource handle over one sealed input.
///
/// Text answers file bytes with reason pairs, sha answers the sealed
/// content hash.
#[derive(Clone, TypedUserData)]
pub struct ResourceHandle {
    /// The sealed store identity.
    #[lua(skip)]
    handle: StoreResource,
    /// Store access for resource reads.
    #[lua(skip)]
    stores: Stores,
}

impl std::fmt::Debug for ResourceHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResourceHandle")
            .field("handle", &self.handle)
            .finish()
    }
}

impl ResourceHandle {
    /// Builds one handle from sealed identity with store access.
    pub fn new(handle: StoreResource, stores: Stores) -> Self {
        Self { handle, stores }
    }

    /// Reads the sealed store identity.
    pub fn handle(&self) -> &StoreResource {
        &self.handle
    }
}

#[typeduserdata_impl]
impl ResourceHandle {
    /// Reads file text, else nil with reason.
    fn text(&self) -> mlua::Result<(Option<String>, Option<String>)> {
        match self.stores.resources().read_text(&self.handle) {
            Ok(found) => Ok((Some(found), None)),
            Err(error) => Ok((None, Some(error.to_string()))),
        }
    }

    /// Reads the sealed content hash.
    fn sha(&self) -> mlua::Result<String> {
        Ok(self.handle.sha().hex())
    }
}
