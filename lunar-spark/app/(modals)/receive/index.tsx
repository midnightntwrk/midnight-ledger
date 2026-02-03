import { useState } from 'react';
import { View, Text, Pressable, Share } from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { useRouter } from 'expo-router';
import { MotiView } from 'moti';
import { X, Copy, Share as ShareIcon, Check } from 'lucide-react-native';
import * as Clipboard from 'expo-clipboard';
import * as Haptics from 'expo-haptics';
import QRCode from 'react-native-qrcode-svg';
import { useWallet } from '../../../src/providers';
import { colors } from '../../../src/design-system';

type WalletType = 'shielded' | 'unshielded' | 'dust';

export default function ReceiveScreen() {
  const router = useRouter();
  const { walletData } = useWallet();
  const [selectedWallet, setSelectedWallet] = useState<WalletType>('shielded');
  const [copied, setCopied] = useState(false);

  const addresses: Record<WalletType, string> = {
    shielded: walletData?.shielded.address ?? '',
    unshielded: walletData?.unshielded.address ?? '',
    dust: walletData?.dust.address ?? '',
  };

  const walletColors: Record<WalletType, string> = {
    shielded: colors.wallet.shielded,
    unshielded: colors.wallet.unshielded,
    dust: colors.wallet.dust,
  };

  const address = addresses[selectedWallet];

  const copyAddress = async () => {
    if (address) {
      await Clipboard.setStringAsync(address);
      Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  };

  const shareAddress = async () => {
    if (address) {
      await Share.share({
        message: address,
        title: `${selectedWallet.charAt(0).toUpperCase() + selectedWallet.slice(1)} Address`,
      });
    }
  };

  return (
    <SafeAreaView className="flex-1 bg-background-primary">
      {/* Header */}
      <View className="flex-row items-center justify-between px-6 py-4">
        <Text className="text-headline-md text-foreground-primary">Receive</Text>
        <Pressable
          className="w-10 h-10 rounded-full bg-background-secondary items-center justify-center active:opacity-60"
          onPress={() => router.back()}
        >
          <X size={20} color={colors.foreground.primary} />
        </Pressable>
      </View>

      <View className="flex-1 px-6">
        {/* Wallet Selector */}
        <MotiView
          from={{ opacity: 0, translateY: 10 }}
          animate={{ opacity: 1, translateY: 0 }}
          transition={{ type: 'timing', duration: 300 }}
          className="flex-row bg-background-secondary rounded-xl p-1 mb-8"
        >
          {(['shielded', 'unshielded', 'dust'] as WalletType[]).map((type) => (
            <Pressable
              key={type}
              className={`flex-1 py-3 rounded-lg items-center ${
                selectedWallet === type ? 'bg-background-tertiary' : ''
              }`}
              onPress={() => setSelectedWallet(type)}
            >
              <Text
                className={`text-label-md capitalize ${
                  selectedWallet === type
                    ? 'text-foreground-primary'
                    : 'text-foreground-tertiary'
                }`}
              >
                {type}
              </Text>
            </Pressable>
          ))}
        </MotiView>

        {/* QR Code */}
        <MotiView
          from={{ opacity: 0, scale: 0.9 }}
          animate={{ opacity: 1, scale: 1 }}
          transition={{ type: 'timing', duration: 400 }}
          className="items-center mb-8"
        >
          <View className="bg-white p-6 rounded-3xl">
            {address ? (
              <QRCode
                value={address}
                size={200}
                color="#000000"
                backgroundColor="#FFFFFF"
              />
            ) : (
              <View className="w-[200px] h-[200px] items-center justify-center">
                <Text className="text-foreground-tertiary">Loading...</Text>
              </View>
            )}
          </View>
          <View
            className="mt-4 px-4 py-2 rounded-full"
            style={{ backgroundColor: `${walletColors[selectedWallet]}20` }}
          >
            <Text
              className="text-label-md capitalize"
              style={{ color: walletColors[selectedWallet] }}
            >
              {selectedWallet} Wallet
            </Text>
          </View>
        </MotiView>

        {/* Address Display */}
        <MotiView
          from={{ opacity: 0, translateY: 10 }}
          animate={{ opacity: 1, translateY: 0 }}
          transition={{ type: 'timing', duration: 400, delay: 200 }}
        >
          <Text className="text-label-sm text-foreground-tertiary mb-2">
            Your {selectedWallet} address
          </Text>
          <View className="bg-background-secondary rounded-xl p-4 border border-border">
            <Text className="text-body-sm text-foreground-secondary font-mono text-center leading-6">
              {address || 'Loading...'}
            </Text>
          </View>
        </MotiView>
      </View>

      {/* Action Buttons */}
      <View className="px-6 pb-6 flex-row gap-4">
        <Pressable
          className="flex-1 bg-background-secondary py-4 rounded-xl flex-row items-center justify-center active:opacity-80"
          onPress={copyAddress}
        >
          {copied ? (
            <Check size={20} color={colors.success.default} />
          ) : (
            <Copy size={20} color={colors.foreground.primary} />
          )}
          <Text className="text-label-md text-foreground-primary ml-2">
            {copied ? 'Copied!' : 'Copy'}
          </Text>
        </Pressable>
        <Pressable
          className="flex-1 bg-accent py-4 rounded-xl flex-row items-center justify-center active:opacity-80"
          onPress={shareAddress}
        >
          <ShareIcon size={20} color={colors.foreground.primary} />
          <Text className="text-label-md text-foreground-primary ml-2">Share</Text>
        </Pressable>
      </View>
    </SafeAreaView>
  );
}
