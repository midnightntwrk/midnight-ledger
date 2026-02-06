require 'json'

package = JSON.parse(File.read(File.join(__dir__, 'package.json')))

Pod::Spec.new do |s|
  s.name         = package['name']
  s.version      = package['version']
  s.summary      = package['description']
  s.homepage     = 'https://github.com/midnight-protocol/react-native-ledger-ffi'
  s.license      = package['license']
  s.author       = package['author']
  s.platform     = :ios, '13.0'
  s.static_framework = true
  s.source       = { :path => '.' }
  s.source_files = 'ios/LedgerFFI.{h,m,swift}', 'ios/LedgerFFI-Bridging-Header.h', 'ios/ledger_uniffi.swift', 'ios/ledger_uniffiFFI.h'
  s.public_header_files = 'ios/*.h', 'ios/LedgerFFI-Bridging-Header.h'
  s.requires_arc = true
  
  # Swift module support
  s.swift_version = '5.0'
  s.pod_target_xcconfig = {
    'SWIFT_INCLUDE_PATHS' => '$(PODS_ROOT)/react-native-ledger-ffi/ios',
    'OTHER_LDFLAGS' => '-force_load $(PODS_ROOT)/react-native-ledger-ffi/ios/libledger_uniffi.a',
    'DEFINES_MODULE' => 'YES'
  }

  s.dependency 'React-Core'

  # Include the Rust library
  s.vendored_libraries = 'ios/libledger_uniffi.a'
  s.library = 'ledger_uniffi'
  s.xcconfig = { 'OTHER_LDFLAGS' => '-force_load $(PODS_ROOT)/react-native-ledger-ffi/ios/libledger_uniffi.a' }

  # Don't install the dependencies when we run `pod install` in the old architecture.
  if ENV['RCT_NEW_ARCH_ENABLED'] == '1' then
    s.compiler_flags = "-DRCT_NEW_ARCH_ENABLED=1"
    s.pod_target_xcconfig    = {
        "HEADER_SEARCH_PATHS" => "\"$(PODS_ROOT)/boost\"",
        "OTHER_CPLUSPLUSFLAGS" => "-DFOLLY_NO_CONFIG -DFOLLY_MOBILE=1 -DFOLLY_USE_LIBCPP=1",
        "CLANG_CXX_LANGUAGE_STANDARD" => "c++17"
    }
    s.dependency "React-RCTFabric"
    s.dependency "React-Codegen"
    s.dependency "RCT-Folly"
    s.dependency "RCTRequired"
    s.dependency "RCTTypeSafety"
    s.dependency "ReactCommon/turbomodule/core"
  end
end
