import * as path from 'node:path';
import * as url from 'node:url';

const currentDir = path.dirname(url.fileURLToPath(import.meta.url));

export default {
  entry: './src/index.js',
  output: {
    filename: 'index.js',
    path: path.resolve(currentDir, 'dist'),
  },
  experiments: {
    asyncWebAssembly: true,
  },
  mode: 'development',
  devServer: {
    headers: {
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Embedder-Policy": "require-corp",
    },
    client: {
      overlay: {
        warnings: false,
      },
    },
  },
  module: {
    rules: [
      {
        test: /\.m?js$/,
        resolve: {
          fullySpecified: false
        }
      }
    ]
  }
};
