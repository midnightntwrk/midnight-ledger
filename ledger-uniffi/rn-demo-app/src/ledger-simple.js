const LedgerFFI = {
  hello: async () => "Hello from Local Ledger Simple! 🚀",
  nativeToken: async () => "Native token placeholder",
  feeToken: async () => "Fee token placeholder",
};

module.exports = LedgerFFI;
module.exports.LedgerFFI = LedgerFFI;
module.exports.default = LedgerFFI;
