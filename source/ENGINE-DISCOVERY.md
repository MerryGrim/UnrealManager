# Existing engines after reinstalling Windows

Version 0.9.13 adds discovery and reuse of engines left on disk after reinstalling Windows.

Unreal Manager scans Epic metadata, Windows engine registrations, its own saved paths,
and common engine folders on fixed local disks. Discovery requires a readable
`Engine/Build/Build.version` and `UnrealEditor.exe` or `UE4Editor.exe`. It reads the
version from the file, regardless of the folder name. The heuristic search has
directory and entry limits; it does not recursively search entire disks.

For another location, open **Engines → Add installed engine** and select the engine
root. This records the path in Unreal Manager settings without an Epic login or
changes to engine files. An incomplete installation without the editor can instead
be selected in the installation wizard for its matching available version.

To verify or complete an existing installation:

1. Sign in to Epic and refresh the available engines.
2. Choose **Verify files** on its installed card, or select its existing root in
   the installation wizard. The picker keeps an engine root as selected; a parent
   directory receives the usual `UE_<branch>` child.
3. Review the component selection and click **Verify and complete**.

For an exact match of version and changelist, old Epic/UM manifests are optional.
The operation hashes every selected existing file, reuses matching files and
downloads missing files. Unknown files outside the manifest remain in place.
Conflicting existing content stops the operation; the application does not silently
replace modified or corrupt files. A different branch, a downgrade, and detected
preview/stable transitions require another folder. Updating an older build still
requires its original manifest to establish which files can safely be replaced.

System prerequisite receipts are bound to the Windows installation using an opaque
hash. A portable settings folder copied from another Windows installation cannot
skip the prerequisites. Windows may request elevation for the official installers.

The available catalogue has **Stable releases**, **Preview versions**, and
**Channel unconfirmed** groups. Channel detection uses explicit build identifiers,
Epic catalogue descriptors, and verified changelists. Epic identified build
`5.9.0-60738873` as [UE 5.9 Preview 2](https://forums.unrealengine.com/t/preview-2-unreal-engine-5-9-preview/2852840?page=3).
The catalogue recognizes that build in older local caches. Other builds from
the same engine branch are evaluated independently. Metadata is cached by the
asset, build, and label without storing credentials.

Validation for 0.9.13: 395 Rust tests passed, with 0 failures and 6 ignored tests.
The engine UI and translation checks passed using synthetic native responses.
The Windows x64 release, installer and portable package were built and their
files checked. The application and Setup were not launched. No real Epic account
transfer, existing user-engine modification or prerequisite execution was
performed in this release check.
