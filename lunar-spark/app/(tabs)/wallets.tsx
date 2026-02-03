import { useState } from 'react';
import { View, Text, Pressable, ScrollView } from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { MotiView, AnimatePresence } from 'moti';
import { ChevronDown, Copy, Eye, EyeOff } from 'lucide-react-native';
import * as Clipboard from 'expo-clipboard';
import * as Haptics from 'expo-haptics';
import { useWallet, type WalletSummary } from '../../src/providers';
import { colors } from '../../src/design-system';
import { formatBalance } from '../../src/utils/balance';
import { getSyncStatusText } from '../../src/utils/sync';

type WalletType = 'shielded' | 'unshielded' | 'dust';

interface WalletCardProps {
  type: WalletType;
  label: string;
  description: string;
  wallet: WalletSummary | undefined;
  accentColor: string;
  tokenName?: string;
}

function WalletCard({ type, label, description, wallet, accentColor, tokenName }: WalletCardProps) {
  const [expanded, setExpanded] = useState(false);
  const [showAddress, setShowAddress] = useState(false);

  const address = wallet?.address ?? '';
  const balance = wallet?.balance ?? 0n;
  const pendingBalance = wallet?.pendingBalance ?? 0n;
  const availableCoins = wallet?.availableCoins ?? 0;
  const pendingCoins = wallet?.pendingCoins ?? 0;
  const syncPercentage = wallet?.syncPercentage ?? 0;

  const copyAddress = async () => {
    if (address) {
      await Clipboard.setStringAsync(address);
      Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
    }
  };

  const truncatedAddress = address
    ? `${address.slice(0, 12)}...${address.slice(-8)}`
    : 'Loading...';

  return (
    <MotiView
      from={{ opacity: 0, translateY: 20 }}
      animate={{ opacity: 1, translateY: 0 }}
      transition={{ type: 'timing', duration: 400 }}
    >
      <Pressable
        className="bg-background-secondary rounded-2xl border border-border overflow-hidden mb-4"
        onPress={() => setExpanded(!expanded)}
      >
        {/* Header */}
        <View className="p-4">
          <View className="flex-row items-center justify-between">
            <View className="flex-row items-center flex-1">
              <View
                className="w-12 h-12 rounded-full items-center justify-center mr-4"
                style={{ backgroundColor: `${accentColor}20` }}
              >
                <View
                  className="w-4 h-4 rounded-full"
                  style={{ backgroundColor: accentColor }}
                />
              </View>
              <View className="flex-1">
                <Text className="text-headline-sm text-foreground-primary">{label}</Text>
                <Text className="text-body-sm text-foreground-tertiary">{description}</Text>
              </View>
            </View>
            <View className="items-end">
              <Text className="text-headline-sm text-foreground-primary">
                {formatBalance(balance)}
              </Text>
              {tokenName && (
                <Text className="text-body-sm text-foreground-tertiary">{tokenName}</Text>
              )}
            </View>
            <MotiView
              animate={{ rotate: expanded ? '180deg' : '0deg' }}
              transition={{ type: 'timing', duration: 200 }}
              className="ml-3"
            >
              <ChevronDown size={20} color={colors.foreground.tertiary} />
            </MotiView>
          </View>

          {/* Sync Progress */}
          <View className="mt-4">
            <View className="flex-row justify-between mb-2">
              <Text className="text-body-sm text-foreground-tertiary">
                {getSyncStatusText(syncPercentage)}
              </Text>
              <Text className="text-body-sm text-foreground-tertiary">{syncPercentage}%</Text>
            </View>
            <View className="h-1.5 bg-background-tertiary rounded-full overflow-hidden">
              <MotiView
                animate={{ width: `${syncPercentage}%` }}
                transition={{ type: 'timing', duration: 300 }}
                className="h-full rounded-full"
                style={{ backgroundColor: syncPercentage === 100 ? colors.success.default : accentColor }}
              />
            </View>
          </View>
        </View>

        {/* Expanded Content */}
        <AnimatePresence>
          {expanded && (
            <MotiView
              from={{ opacity: 0, height: 0 }}
              animate={{ opacity: 1, height: 'auto' }}
              exit={{ opacity: 0, height: 0 }}
              transition={{ type: 'timing', duration: 200 }}
            >
              <View className="border-t border-border px-4 py-4">
                {/* Address */}
                <View className="mb-4">
                  <Text className="text-label-sm text-foreground-tertiary mb-2">Address</Text>
                  <View className="flex-row items-center bg-background-tertiary rounded-lg p-3">
                    <Text className="flex-1 text-body-sm text-foreground-secondary font-mono">
                      {showAddress ? address : truncatedAddress}
                    </Text>
                    <Pressable
                      className="p-2 active:opacity-60"
                      onPress={() => setShowAddress(!showAddress)}
                    >
                      {showAddress ? (
                        <EyeOff size={18} color={colors.foreground.tertiary} />
                      ) : (
                        <Eye size={18} color={colors.foreground.tertiary} />
                      )}
                    </Pressable>
                    <Pressable className="p-2 active:opacity-60" onPress={copyAddress}>
                      <Copy size={18} color={colors.foreground.tertiary} />
                    </Pressable>
                  </View>
                </View>

                {/* Balances */}
                <View className="flex-row gap-4">
                  <View className="flex-1 bg-background-tertiary rounded-lg p-3">
                    <Text className="text-label-sm text-foreground-tertiary mb-1">Available</Text>
                    <Text className="text-label-lg text-foreground-primary">
                      {formatBalance(balance)}
                    </Text>
                    <Text className="text-body-sm text-foreground-tertiary">
                      {availableCoins} coins
                    </Text>
                  </View>
                  <View className="flex-1 bg-background-tertiary rounded-lg p-3">
                    <Text className="text-label-sm text-foreground-tertiary mb-1">Pending</Text>
                    <Text className="text-label-lg text-warning">
                      {formatBalance(pendingBalance)}
                    </Text>
                    <Text className="text-body-sm text-foreground-tertiary">
                      {pendingCoins} coins
                    </Text>
                  </View>
                </View>
              </View>
            </MotiView>
          )}
        </AnimatePresence>
      </Pressable>
    </MotiView>
  );
}

export default function WalletsScreen() {
  const { walletData } = useWallet();

  return (
    <SafeAreaView className="flex-1 bg-background-primary" edges={['top']}>
      <ScrollView className="flex-1 px-6" showsVerticalScrollIndicator={false}>
        {/* Header */}
        <View className="pt-4 pb-6">
          <Text className="text-headline-lg text-foreground-primary">Wallets</Text>
          <Text className="text-body-md text-foreground-secondary mt-1">
            Manage your wallet addresses and balances
          </Text>
        </View>

        {/* Wallet Cards */}
        <WalletCard
          type="shielded"
          label="Shielded"
          description="Private transactions"
          wallet={walletData?.shielded}
          accentColor={colors.wallet.shielded}
        />

        <WalletCard
          type="unshielded"
          label="Unshielded"
          description="Public transactions"
          wallet={walletData?.unshielded}
          accentColor={colors.wallet.unshielded}
          tokenName="tNIGHT"
        />

        <WalletCard
          type="dust"
          label="Dust"
          description="Network fees"
          wallet={walletData?.dust}
          accentColor={colors.wallet.dust}
          tokenName="tDUST"
        />

        {/* Bottom spacing */}
        <View className="h-8" />
      </ScrollView>
    </SafeAreaView>
  );
}
