const path = require('path');

module.exports = {
  dependencies: {
    'react-native-ledger-ffi': {
      platforms: {
        android: {
          sourceDir: '../react-native-ledger-ffi/android',
          packageImportPath: 'import com.midnight.ledgerffi.LedgerFFIPackage;',
          packageInstance: 'new LedgerFFIPackage()',
        },
        ios: {
          podspecPath: path.resolve(__dirname, '../react-native-ledger-ffi/react-native-ledger-ffi.podspec'),
        },
      },
    },
  },
};
