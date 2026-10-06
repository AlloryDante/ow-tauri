const path = require('path');
/* eslint-disable @typescript-eslint/no-var-requires */
const webpack = require('webpack');
const HtmlWebpackPlugin = require('html-webpack-plugin');
const config = require('./webpack.base.config');

const mainConfig = { ...config };
// ow-tauri: the main process runs in the hidden main webview (ow-main).
mainConfig.target = 'web';
mainConfig.entry = {
  index: './src/browser/index.ts',
};

mainConfig.output = {
  path: path.join(__dirname, './dist/browser'),
  filename: '[name].js',
};

// ow-tauri: Node built-ins the main process uses (docs/PORT-MAP.md section 3).
mainConfig.resolve = {
  ...config.resolve,
  fallback: {
    path: require.resolve('path-browserify'),
    events: require.resolve('events/'),
    fs: false,
    child_process: false,
  },
};

// `__dirname` is the app-asset folder of this bundle, so
// path.join(__dirname, '../renderer/index.html') names an app asset.
mainConfig.node = { __dirname: false };

mainConfig.plugins = [
  new webpack.DefinePlugin({ __dirname: JSON.stringify('/browser') }),
  // The page the plugin loads into the main webview (plugins.overwolf.main.url).
  new HtmlWebpackPlugin({
    title: 'main',
    filename: path.join(__dirname, './dist/browser/main.html'),
    chunks: ['index'],
    inject: true,
  }),
];

module.exports = mainConfig;
