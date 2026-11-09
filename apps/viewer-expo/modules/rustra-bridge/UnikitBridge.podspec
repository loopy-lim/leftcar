# ── unikit generated ────────────────────────────────────────
# File:   UnikitBridge.podspec
# Source: schema.json (single source of truth for this file)
# Regen:  unikit codegen --config unikit.json
# Stage:  rust-probe schema → ts renderer
# DO NOT EDIT — changes will be overwritten and fail codegen --check.
# ────────────────────────────────────────────────────────────

Pod::Spec.new do |s|
  s.name = 'UnikitBridge'
  s.version = '0.0.0'
  s.summary = 'Generated Unikit JSI bridge'
  s.author = 'Unikit contributors'
  s.homepage = 'https://github.com/loopy-lim/unikit-bridge'
  s.license = 'MIT'
  s.platforms = { :ios => '15.1' }
  s.source = { :path => '.' }
  s.static_framework = true

  adapter_root = File.expand_path('../../node_modules/@unikit-bridge/react-native/native', __dir__)
  generated_root = File.expand_path('generated', __dir__)
  rust_archive = 'ios/rust/lib/libleftcar_rustra.a'

  s.prepare_command = 'sh ios/build-rust-ios.sh'
  s.vendored_libraries = rust_archive
  s.source_files = 'ios/UnikitBridge*.{mm,cpp}'
  s.dependency 'React-jsi'
  s.dependency 'React-Core'
  install_modules_dependencies(s)

  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
    'HEADER_SEARCH_PATHS' => "$(inherited) #{adapter_root}/cpp #{adapter_root}/ios #{generated_root}",
    'OTHER_LDFLAGS' => "$(inherited) -force_load $(PODS_TARGET_SRCROOT)/#{rust_archive}",
    'CLANG_CXX_LANGUAGE_STANDARD' => 'c++20',
  }
end
