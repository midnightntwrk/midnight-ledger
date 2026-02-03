module.exports = function (api) {
  api.cache(true);
  return {
    presets: [
      [
        'babel-preset-expo',
        {
          unstable_transformImportMeta: true,
          lazyImports: true,
        },
      ],
      'nativewind/babel',
    ],
    plugins: [
      [
        'module-resolver',
        {
          alias: {
            crypto: 'react-native-quick-crypto',
            stream: 'readable-stream',
            buffer: '@craftzdog/react-native-buffer',
            assert: 'assert',
            util: 'util',
            events: 'events',
            url: 'url',
            path: 'path-browserify',
          },
        },
      ],
      // Use loose mode for CommonJS transform to avoid frozen default errors
      [
        '@babel/plugin-transform-modules-commonjs',
        {
          loose: true,
          allowTopLevelThis: true,
        },
      ],
    ],
  };
};
