import { Zkir } from '@midnight-ntwrk/zkir-v2';
import { describe, it, expect } from 'vitest';
import { keyMaterialProvider } from '../../test-objects';

const keys = [
  { keyLocation: 'midnight/zswap/spend', k: 15 },
  { keyLocation: 'midnight/zswap/output', k: 14 },
  { keyLocation: 'midnight/zswap/sign', k: 13 },
  { keyLocation: 'midnight/dust/spend', k: 13 }
];

describe('ZKIRV2', () => {
  it.concurrent.each(keys)('reports the correct k value for $keyLocation', async ({ keyLocation, k }) => {
    const rawKeyMaterial = await keyMaterialProvider.lookupKey(keyLocation);

    const zkir = Zkir.deserialize(rawKeyMaterial!.ir);

    expect(zkir.getK()).toBe(k);
  });

  it.concurrent.each(keys)('can serialize and deserialize a Zkir of $keyLocation', async ({ keyLocation }) => {
    const jsonZkir = await keyMaterialProvider.lookupJsonIr(keyLocation);
    const serializedFromJson = Zkir.fromJson(jsonZkir!).serialize();

    const serializedLoaded = (await keyMaterialProvider.lookupKey(keyLocation))!.ir;

    expect(Buffer.from(serializedFromJson)).toEqual(serializedLoaded);
  });
});
