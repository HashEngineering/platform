const merkleJSON = {
  merkleBlock: { // Mainnet Block 300100
    header: {
      hash: '00000000000051b99f3fa12da6997bb54327c7b81509829467b499a7ce03136a',
      version: 3,
      prevHash: '0000000000154ef1f88653a6757f1182680a4d5831d815010d5fa646ae79ce35',
      merkleRoot: 'a0055d45ad9b35e77fb01c59a4feb9976921493d2557a5ac0798b49e82ea1e99',
      time: 1436550250,
      bits: 454590659,
      nonce: 2601264896,
    },
    numTransactions: 12,
    hashes: [
      '9d0a368bc9923c6cb966135a4ceda30cc5f259f72c8843ce015056375f8a06ec',
      '39e5cd533567ac0a8602bcc4c29e2f01a4abb0fe68ffbc7be6c393db188b72e0',
      'cd75b421157eca03eff664bdc165730f91ef2fa52df19ff415ab5acb30045425',
      '2ef9795147caaeecee5bc2520704bb372cde06dbd2e871750f31336fd3f02be3',
      '2241d3448560f8b1d3a07ea5c31e79eb595632984a20f50944809a61fdd9fe0b',
      '45afbfe270014d5593cb065562f1fed726f767fe334d8b3f4379025cfa5be8c5',
      '198c03da0ccf871db91fe436e2795908eac5cc7d164232182e9445f7f9db1ab2',
      'ed07c181ce5ba7cb66d205bc970f43e1ca11996d611aa8e91e305eb8608c543c',
    ],
    flags: [219, 63],
  },
  // merkleblock for inserted tx hash
  // '7262476912a96b9a6226cfa3a8f231ba3e2b1f75c396e88367e532c79c43c95b' in testnet block 10000
  // (bloom filter: '03359be1100000000000000001')
  rawMerkleBlock: '000000200e71587f863213690c24b8ad07d0753b38abe6ff1328ef7661689404000000000f1e7ce895614047d5c8d3e7b537b26b0482302301aa104947eb09c55521b316b10b1e5cff7d191c166b25b80500000002a8f781d45a5b12abffcf52bac3b3fc7824cc91ef4f9bb04bc97500a1d98d91305bc9439cc732e56783e896c3751f2b3eba31f2a8a3cf26629a6ba91269476272011d',
  // merkleblock for inserted tx hashes
  // '7262476912a96b9a6226cfa3a8f231ba3e2b1f75c396e88367e532c79c43c95b'
  // '3f3517ee8fa95621fe8abdd81c1e0dfb50e21dd4c5a3c01eee2c47cf664821b6' in testnet block 10000
  // (bloom filter: '0797b88e7d21b31f130000000000000001')
  rawMerkleBlock2: '000000200e71587f863213690c24b8ad07d0753b38abe6ff1328ef7661689404000000000f1e7ce895614047d5c8d3e7b537b26b0482302301aa104947eb09c55521b316b10b1e5cff7d191c166b25b80500000004acff07e45bfb50dd8e2fb10109f1988423cbcc9423ff07b7acb09715bd1a11e89153210725b8e219b91e433180092226c7b56649843fc51bc012c587f50030a1b6214866cf472cee1ec0a3c5d41de250fb0d1e1cd8bd8afe2156a98fee17353f5bc9439cc732e56783e896c3751f2b3eba31f2a8a3cf26629a6ba9126947627202eb01',
};

module.exports = merkleJSON;