fn main() {
    tauri_build::build();

    // Embed an explicit application manifest so the OS does NOT run our
    // .exe under the legacy compat shim. The shim forces GetVersionExW
    // to return build 9200 (Win8) which breaks any sane OS version check.
    //
    // Supported OS GUIDs below are the canonical set used by Microsoft
    // tools (Visual C++ team, Edge, WebView2 distros):
    //   Win10/11:    8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a
    //   Win8.1:      1f676c76-80e1-4239-95bb-83d0f6d0da78
    //   Win8:        4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38
    //
    // We are intentionally claiming support down to Win8 to keep the
    // preflight reporting accurate. The preflight itself enforces a
    // higher minimum (17763 / Win10 1809) for actual functionality.

    let manifest = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity
      type="win32"
      name="InkUO.App"
      version="0.1.0.0"
      processorArchitecture="*"/>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges xmlns="urn:schemas-microsoft-com:asm.v3">
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <!-- Windows 10 / 11 -->
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
      <!-- Windows 8.1 -->
      <supportedOS Id="{1f676c76-80e1-4239-95bb-83d0f6d0da78}"/>
      <!-- Windows 8 -->
      <supportedOS Id="{4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
    </windowsSettings>
  </application>
</assembly>
"#;

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set by cargo");
    let manifest_path = std::path::Path::new(&out_dir).join("app.manifest");
    std::fs::write(&manifest_path, manifest).expect("failed to write app.manifest");

    embed_resource::compile(&manifest_path, embed_resource::NONE);
}