# Builds yavcpdf from this checkout. Used by flake.nix; also works with `callPackage`.
{
  lib,
  rustPlatform,
  makeWrapper,
  libGL,
  libxkbcommon,
  wayland,
  libx11,
  libxcursor,
  libxi,
  libxrender,
  dbus,
  fontconfig,
  xdg-utils,
}:

let
  root = ../..;
  # eframe/winit/glutin dlopen these at run time rather than linking them, so the wrapper puts
  # them on LD_LIBRARY_PATH (the same list as the AUR package's depends).
  runtimeLibs = [
    libGL
    libxkbcommon
    wayland
    libx11
    libxcursor
    libxi
    libxrender
    dbus
  ];

  # pdfcraft-fonts embeds fonts from assets/fonts at the root of the PdfCraft repository, outside
  # its own crate, which Nix's vendoring leaves out. Fetch the repository at the rev Cargo.lock
  # names (so scripts/update-pdfcraft.sh keeps it in step) to put them back.
  lock = lib.importTOML (root + "/Cargo.lock");
  pdfcraftFonts = lib.findFirst (p: p.name == "pdfcraft-fonts") (throw "pdfcraft-fonts is not in Cargo.lock") lock.package;
  pdfcraft = builtins.fetchGit {
    url = "https://github.com/storytold/pdfcraft";
    rev = lib.last (lib.splitString "#" pdfcraftFonts.source);
    allRefs = true;
  };
in
rustPlatform.buildRustPackage {
  pname = "yavcpdf";
  version = (lib.importTOML (root + "/Cargo.toml")).package.version;

  src = lib.fileset.toSource {
    inherit root;
    fileset = lib.fileset.unions [
      (root + "/Cargo.toml")
      (root + "/Cargo.lock")
      (root + "/src")
      (root + "/assets")
      (root + "/README.md")
      (root + "/LICENSE-MIT")
      (root + "/LICENSE-APACHE")
      (root + "/NOTICE")
      (root + "/ATTRIBUTION.md")
      (root + "/licenses")
    ];
  };

  # PdfCraft and the egui and winit forks are git dependencies; let Nix fetch them from Cargo.lock
  # instead of listing an output hash for each.
  cargoLock = {
    lockFile = root + "/Cargo.lock";
    allowBuiltinFetchGit = true;
  };

  nativeBuildInputs = [ makeWrapper ];

  # The vendored crate looks three levels up from its src/, which is the build directory.
  preBuild = ''
    mkdir -p "$NIX_BUILD_TOP/assets"
    cp -r ${pdfcraft}/assets/fonts "$NIX_BUILD_TOP/assets/"
  '';

  postInstall = ''
    install -Dm644 assets/yavcpdf.desktop -t $out/share/applications
    install -Dm644 assets/yavcpdf.svg -t $out/share/icons/hicolor/scalable/apps
    install -Dm644 README.md -t $out/share/doc/yavcpdf
    install -Dm644 src/default_config.toml $out/share/doc/yavcpdf/config.toml
    install -Dm644 LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md licenses/* -t $out/share/licenses/yavcpdf
  '';

  # fc-match is run for the `font` setting and xdg-open for links. They go at the end of PATH, so
  # the system's own xdg-open (and its browser settings) still wins where there is one.
  postFixup = ''
    wrapProgram $out/bin/yavcpdf \
      --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibs} \
      --suffix PATH : ${
        lib.makeBinPath [
          fontconfig
          xdg-utils
        ]
      }
  '';

  meta = {
    description = "A minimal, keyboard-driven PDF viewer";
    homepage = "https://github.com/araaha/yavcpdf";
    license = with lib.licenses; [
      mit
      asl20
    ];
    mainProgram = "yavcpdf";
    platforms = lib.platforms.linux;
  };
}
