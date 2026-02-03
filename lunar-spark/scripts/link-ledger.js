const fs = require('fs');
const path = require('path');

const nodeModulesPath = path.resolve(__dirname, '../node_modules/@midnight-ntwrk');
const expoLedgerSource = path.resolve(__dirname, '../../expo-midnight-ledger');

// Ensure the @midnight-ntwrk directory exists
if (!fs.existsSync(nodeModulesPath)) {
  fs.mkdirSync(nodeModulesPath, {recursive: true});
}

function createLink(linkPath, targetPath, name) {
  try {
    // Check if path exists
    const stats = fs.lstatSync(linkPath);

    // If it's already a correct symlink, skip
    if (stats.isSymbolicLink()) {
      const currentTarget = fs.readlinkSync(linkPath);
      if (path.resolve(path.dirname(linkPath), currentTarget) === targetPath) {
        console.log(`Symlink already correct: ${name}`);
        return;
      }
    }

    // Remove existing file/directory/symlink
    fs.rmSync(linkPath, {recursive: true, force: true});
  } catch (e) {
    // Path doesn't exist, which is fine
  }

  // Create the symlink
  fs.symlinkSync(targetPath, linkPath);
  console.log(`Created symlink: ${name} -> expo-midnight-ledger`);
}

// Create symlink for expo-midnight-ledger
createLink(
  path.join(nodeModulesPath, 'expo-midnight-ledger'),
  expoLedgerSource,
  '@midnight-ntwrk/expo-midnight-ledger'
);

// Create symlink for ledger-v7 (alias to expo-midnight-ledger)
createLink(path.join(nodeModulesPath, 'ledger-v7'), expoLedgerSource, '@midnight-ntwrk/ledger-v7');
