import { View, Text, Pressable, ScrollView } from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { useRouter } from 'expo-router';
import { MotiView } from 'moti';
import { Send, QrCode } from 'lucide-react-native';
import { useWallet } from '../../src/providers';
import { colors } from '../../src/design-system';
import { formatBalance } from '../../src/utils/balance';

export default function HomeScreen() {
  const router = useRouter();
  const { walletData, environment } = useWallet();

  const totalBalance = walletData?.totalBalance ?? 0n;
  const isSynced = walletData?.isSynced ?? false;

  return (
    <SafeAreaView className="flex-1 bg-background-primary" edges={['top']}>
      <ScrollView className="flex-1" showsVerticalScrollIndicator={false}>
        {/* Header */}
        <View className="px-6 pt-4 pb-2">
          <View className="flex-row items-center justify-between">
            <View>
              <Text className="text-body-sm text-foreground-secondary">Portfolio Value</Text>
              <View className="flex-row items-center mt-1">
                <View
                  className={`w-2 h-2 rounded-full mr-2 ${isSynced ? 'bg-success' : 'bg-warning'}`}
                />
                <Text className="text-body-sm text-foreground-tertiary">
                  {isSynced ? 'Synced' : 'Syncing...'}
                </Text>
              </View>
            </View>
            <View className="bg-accent/20 px-3 py-1 rounded-full">
              <Text className="text-label-sm text-accent uppercase">{environment}</Text>
            </View>
          </View>
        </View>

        {/* Balance Card */}
        <MotiView
          from={{ opacity: 0, translateY: 20 }}
          animate={{ opacity: 1, translateY: 0 }}
          transition={{ type: 'timing', duration: 500 }}
          className="mx-6 mt-4"
        >
          <View className="bg-background-secondary rounded-2xl p-6 border border-border">
            <Text className="text-display-md text-foreground-primary">
              {formatBalance(totalBalance)}
            </Text>
            <Text className="text-body-md text-foreground-secondary mt-1">Total Balance</Text>

            {/* Quick Actions */}
            <View className="flex-row mt-6 gap-3">
              <Pressable
                className="flex-1 bg-accent py-3 rounded-xl flex-row items-center justify-center active:opacity-80"
                onPress={() => router.push('/(modals)/send')}
              >
                <Send size={18} color={colors.foreground.primary} strokeWidth={2} />
                <Text className="text-label-md text-foreground-primary ml-2">Send</Text>
              </Pressable>
              <Pressable
                className="flex-1 bg-background-tertiary py-3 rounded-xl flex-row items-center justify-center active:opacity-80"
                onPress={() => router.push('/(modals)/receive')}
              >
                <QrCode size={18} color={colors.foreground.primary} strokeWidth={2} />
                <Text className="text-label-md text-foreground-primary ml-2">Receive</Text>
              </Pressable>
            </View>
          </View>
        </MotiView>

        {/* Wallet Overview */}
        <View className="px-6 mt-8">
          <Text className="text-headline-sm text-foreground-primary mb-4">Your Wallets</Text>

          {/* Shielded */}
          <MotiView
            from={{ opacity: 0, translateX: -20 }}
            animate={{ opacity: 1, translateX: 0 }}
            transition={{ type: 'timing', duration: 400, delay: 100 }}
          >
            <Pressable
              className="bg-background-secondary rounded-xl p-4 border border-border mb-3 active:opacity-80"
              onPress={() => router.push('/(tabs)/wallets')}
            >
              <View className="flex-row items-center justify-between">
                <View className="flex-row items-center">
                  <View className="w-10 h-10 rounded-full bg-wallet-shielded/20 items-center justify-center mr-3">
                    <View className="w-3 h-3 rounded-full bg-wallet-shielded" />
                  </View>
                  <View>
                    <Text className="text-label-lg text-foreground-primary">Shielded</Text>
                    <Text className="text-body-sm text-foreground-tertiary">Private transactions</Text>
                  </View>
                </View>
                <View className="items-end">
                  <Text className="text-label-lg text-foreground-primary">
                    {formatBalance(walletData?.shielded.balance ?? 0n)}
                  </Text>
                </View>
              </View>
            </Pressable>
          </MotiView>

          {/* Unshielded */}
          <MotiView
            from={{ opacity: 0, translateX: -20 }}
            animate={{ opacity: 1, translateX: 0 }}
            transition={{ type: 'timing', duration: 400, delay: 200 }}
          >
            <Pressable
              className="bg-background-secondary rounded-xl p-4 border border-border mb-3 active:opacity-80"
              onPress={() => router.push('/(tabs)/wallets')}
            >
              <View className="flex-row items-center justify-between">
                <View className="flex-row items-center">
                  <View className="w-10 h-10 rounded-full bg-wallet-unshielded/20 items-center justify-center mr-3">
                    <View className="w-3 h-3 rounded-full bg-wallet-unshielded" />
                  </View>
                  <View>
                    <Text className="text-label-lg text-foreground-primary">Unshielded</Text>
                    <Text className="text-body-sm text-foreground-tertiary">Public transactions</Text>
                  </View>
                </View>
                <View className="items-end">
                  <Text className="text-label-lg text-foreground-primary">
                    {formatBalance(walletData?.unshielded.balance ?? 0n)}
                  </Text>
                  <Text className="text-body-sm text-foreground-tertiary">tNIGHT</Text>
                </View>
              </View>
            </Pressable>
          </MotiView>

          {/* Dust */}
          <MotiView
            from={{ opacity: 0, translateX: -20 }}
            animate={{ opacity: 1, translateX: 0 }}
            transition={{ type: 'timing', duration: 400, delay: 300 }}
          >
            <Pressable
              className="bg-background-secondary rounded-xl p-4 border border-border active:opacity-80"
              onPress={() => router.push('/(tabs)/wallets')}
            >
              <View className="flex-row items-center justify-between">
                <View className="flex-row items-center">
                  <View className="w-10 h-10 rounded-full bg-wallet-dust/20 items-center justify-center mr-3">
                    <View className="w-3 h-3 rounded-full bg-wallet-dust" />
                  </View>
                  <View>
                    <Text className="text-label-lg text-foreground-primary">Dust</Text>
                    <Text className="text-body-sm text-foreground-tertiary">Network fees</Text>
                  </View>
                </View>
                <View className="items-end">
                  <Text className="text-label-lg text-foreground-primary">
                    {formatBalance(walletData?.dust.balance ?? 0n)}
                  </Text>
                  <Text className="text-body-sm text-foreground-tertiary">tDUST</Text>
                </View>
              </View>
            </Pressable>
          </MotiView>
        </View>

        {/* Bottom spacing */}
        <View className="h-8" />
      </ScrollView>
    </SafeAreaView>
  );
}
