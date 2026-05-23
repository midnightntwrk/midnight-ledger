const { getDefaultConfig, mergeConfig } = require("@react-native/metro-config");
const path = require("path");

// Metro doesn't know how to resolve `file:../react-native-prover`
// out of the box — we need to whitelist the parent directory so
// Metro will follow symlinks / file-protocol deps into it.
const projectRoot = __dirname;
const proverRoot = path.resolve(projectRoot, "..", "react-native-prover");

const config = {
  watchFolders: [proverRoot],
  resolver: {
    // Hoist the shared node_modules of the demo over the prover's
    // (which has no node_modules until the consumer installs it).
    nodeModulesPaths: [path.resolve(projectRoot, "node_modules")],
    // Allow `import { prove } from "@midnight-ntwrk/react-native-prover"`
    // to resolve to the prover package's source tree directly during
    // local dev. In production the package would be installed from
    // npm and this alias becomes redundant.
    extraNodeModules: {
      "@midnight-ntwrk/react-native-prover": proverRoot,
    },
  },
};

module.exports = mergeConfig(getDefaultConfig(__dirname), config);
