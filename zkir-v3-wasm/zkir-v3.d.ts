
export type ProvingProvider = {
  check(
    serializedPreimage: Uint8Array,
    keyLocation: string,
  ): Promise<(bigint | undefined)[]>;
  prove(
    serializedPreimage: Uint8Array,
    keyLocation: string,
    overwriteBindingInput?: bigint,
  ): Promise<Uint8Array>;
};

export type ProvingKeyMaterial = {
  proverKey: Uint8Array,
  verifierKey: Uint8Array,
  ir: Uint8Array,
};

export type KeyMaterialProvider = {
    lookupKey(keyLocation: string): Promise<ProvingKeyMaterial | undefined>;
    getParams(k: number): Promise<Uint8Array>;
}

export function prove(
    serializedPreimage: Uint8Array,
    provider: KeyMaterialProvider,
    overwriteBindingInput?: bigint,
): Promise<Uint8Array>;

export function check(
    serializedPreimage: Uint8Array,
    provider: KeyMaterialProvider,
): Promise<(bigint | undefined)[]>;

export function provingProvider(kmProvider: KeyMaterialProvider): ProvingProvider;

export function jsonIrToBinary(json: String): Uint8Array;
