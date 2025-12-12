module.exports = {
  dependencies: {
    'react-native-ledger-ffi': {
      platforms: {
        android: {
          sourceDir: './android',
          packageImportPath: 'import com.midnight.ledgerffi.LedgerFFIPackage;',
          packageInstance: 'new LedgerFFIPackage()',
        },
        ios: {
          podspecPath: './react-native-ledger-ffi.podspec',
          sharedLibraries: ['libledger_uniffi'],
        },
      },
    },
  },
};
