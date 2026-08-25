module.exports = {
  dependency: {
    platforms: {
      ios: { podspecPath: './RustraBridge.podspec' },
      android: {
        sourceDir: './android',
        packageImportPath: 'import dev.rustra.bridge.RustraBridgePackage;',
        packageInstance: 'new RustraBridgePackage()',
      },
    },
  },
};
