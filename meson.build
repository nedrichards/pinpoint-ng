project(
  'pinpoint',
  version: '0.3.0',
  meson_version: '>= 1.4.0',
  license: 'LGPL-2.1-or-later',
)

gnome = import('gnome')
cargo = find_program('cargo', '/usr/lib/sdk/rust-stable/bin/cargo')
cargo_build = find_program('scripts/cargo-build.sh')

# Meson owns the target directory in every context. Build and checks inherit
# exactly the same environment; Flatpak supplies its offline Cargo home.
cargo_env = environment()
cargo_env.set('CARGO_TARGET_DIR', meson.project_build_root() / 'target')
cargo_profile = get_option('buildtype') == 'release' ? 'release' : 'dev'
cargo_args = ['--locked', '--profile', cargo_profile]

binary = custom_target(
  'pinpoint-binary',
  output: 'pinpoint',
  command: [cargo_build, cargo.full_path(), meson.project_source_root(), cargo_profile, '@OUTPUT@'],
  env: cargo_env,
  # Cargo tracks Rust sources, include_str! assets and dependency changes.
  build_always_stale: true,
  build_by_default: true,
  install: true,
  install_dir: get_option('bindir'),
)

test(
  'rust-unit-tests', cargo,
  args: ['test', '--workspace', '--exclude', 'pinpoint-core'] + cargo_args,
  env: cargo_env,
  workdir: meson.project_source_root(),
  is_parallel: false,
  timeout: 600,
)
# The core suite includes a timing gate that requires optimized code.
test(
  'rust-core-tests', cargo,
  args: ['test', '--locked', '--release', '-p', 'pinpoint-core'],
  env: cargo_env,
  workdir: meson.project_source_root(),
  is_parallel: false,
  timeout: 300,
)
test(
  'rust-format', cargo,
  args: ['fmt', '--all', '--', '--check'],
  env: cargo_env,
  workdir: meson.project_source_root(),
  is_parallel: false,
)
test(
  'rust-clippy', cargo,
  args: ['clippy', '--workspace', '--all-targets'] + cargo_args + ['--', '-D', 'warnings'],
  env: cargo_env,
  workdir: meson.project_source_root(),
  is_parallel: false,
  timeout: 600,
)

install_data(
  'data/pinpoint.lang',
  install_dir: get_option('datadir') / 'gtksourceview-5/language-specs',
)
install_data(
  'data/pinpoint-style.xml', 'data/pinpoint-dark-style.xml',
  install_dir: get_option('datadir') / 'gtksourceview-5/styles',
)
install_subdir('data/introduction', install_dir: get_option('datadir') / 'pinpoint')
install_data(
  'pinpoint/data/com.nedrichards.pinpoint.desktop',
  install_dir: get_option('datadir') / 'applications',
)
install_data(
  'pinpoint/data/com.nedrichards.pinpoint.metainfo.xml',
  install_dir: get_option('datadir') / 'metainfo',
)
install_data(
  'pinpoint/data/com.nedrichards.pinpoint.gschema.xml',
  install_dir: get_option('datadir') / 'glib-2.0/schemas',
)
install_data(
  'data/com.nedrichards.pinpoint.svg',
  install_dir: get_option('datadir') / 'icons/hicolor/scalable/apps',
)
install_data(
  'data/com.nedrichards.pinpoint.xml',
  install_dir: get_option('datadir') / 'mime/packages',
)
gnome.post_install(glib_compile_schemas: true, gtk_update_icon_cache: true, update_mime_database: true)

summary({
  'Cargo profile': cargo_profile,
  'Cargo target directory': meson.project_build_root() / 'target',
  'Core test profile': 'release',
}, section: 'Pinpoint build context')
