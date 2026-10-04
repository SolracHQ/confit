//! Handles
//! Opened resource values shared by every opener.

use mlua_extras::TypedUserData;
use mlua_extras::typeduserdata_impl;

use confit_store::Stores;
use confit_store::handles::ResourceHandle as StoreResource;
use confit_store::handles::TrustedHandle;

/// Resource handle over one opened input.
#[derive(Clone, TypedUserData)]
pub struct Handle {
    /// The opened store identity.
    #[lua(skip)]
    handle: StoreResource,
    /// Store access for resource reads.
    #[lua(skip)]
    stores: Stores,
}

impl std::fmt::Debug for Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handle")
            .field("handle", &self.handle)
            .finish()
    }
}

impl PartialEq for Handle {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
    }
}

impl Handle {
    /// Builds one handle from opened identity with store access.
    pub fn new(handle: StoreResource, stores: Stores) -> Self {
        Self { handle, stores }
    }

    /// Reads the opened store identity.
    pub fn handle(&self) -> &StoreResource {
        &self.handle
    }
}

confit_surface_face::handle_schema!(Handle);

#[typeduserdata_impl]
impl Handle {
    /// Reads file text, else nil with reason.
    fn text(&self) -> mlua::Result<(Option<String>, Option<String>)> {
        match self.stores.resources().read_text(&self.handle) {
            Ok(found) => Ok((Some(found), None)),
            Err(error) => Ok((None, Some(error.to_string()))),
        }
    }

    /// Renders the opened content hash.
    fn sha(&self) -> mlua::Result<String> {
        Ok(self.handle.sha().hex())
    }
}
