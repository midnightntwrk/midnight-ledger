import { useState } from 'react';
import { View, Text, TextInput, Pressable, KeyboardAvoidingView, Platform } from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { useRouter } from 'expo-router';
import { MotiView } from 'moti';
import { X, ChevronRight } from 'lucide-react-native';
import { useWallet } from '../../../src/providers';
import { colors } from '../../../src/design-system';
import { formatBalance } from '../../../src/utils/balance';

type WalletType = 'shielded' | 'unshielded';

export default function SendScreen() {
  const router = useRouter();
  const { walletData } = useWallet();
  const [selectedWallet, setSelectedWallet] = useState<WalletType>('shielded');
  const [amount, setAmount] = useState('');

  const balances: Record<WalletType, bigint> = {
    shielded: walletData?.shielded.balance ?? 0n,
    unshielded: walletData?.unshielded.balance ?? 0n,
  };

  const walletColors: Record<WalletType, string> = {
    shielded: colors.wallet.shielded,
    unshielded: colors.wallet.unshielded,
  };

  const tokenNames: Record<WalletType, string> = {
    shielded: '',
    unshielded: 'tNIGHT',
  };

  const availableBalance = balances[selectedWallet];
  const tokenName = tokenNames[selectedWallet];
  const parsedAmount = amount ? BigInt(Math.floor(parseFloat(amount) * 1e6)) : 0n;
  const isValidAmount = parsedAmount > 0n && parsedAmount <= availableBalance;

  const handleMaxPress = () => {
    const balanceString = (Number(availableBalance) / 1e6).toString();
    setAmount(balanceString);
  };

  const handleContinue = () => {
    router.push({
      pathname: '/(modals)/send/recipient',
      params: {
        wallet: selectedWallet,
        amount: parsedAmount.toString(),
      },
    });
  };

  return (
    <SafeAreaView className="flex-1 bg-background-primary">
      <KeyboardAvoidingView
        className="flex-1"
        behavior={Platform.OS === 'ios' ? 'padding' : 'height'}
      >
        {/* Header */}
        <View className="flex-row items-center justify-between px-6 py-4">
          <Text className="text-headline-md text-foreground-primary">Send</Text>
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
            className="mb-6"
          >
            <Text className="text-label-sm text-foreground-tertiary mb-3">From</Text>
            <View className="flex-row bg-background-secondary rounded-xl p-1">
              {(['shielded', 'unshielded'] as WalletType[]).map((type) => (
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
            </View>
            <View className="flex-row justify-between mt-3">
              <Text className="text-body-sm text-foreground-tertiary">Available</Text>
              <Text className="text-body-sm text-foreground-secondary">
                {formatBalance(availableBalance)}{tokenName ? ` ${tokenName}` : ''}
              </Text>
            </View>
          </MotiView>

          {/* Amount Input */}
          <MotiView
            from={{ opacity: 0, translateY: 10 }}
            animate={{ opacity: 1, translateY: 0 }}
            transition={{ type: 'timing', duration: 300, delay: 100 }}
            className="mb-6"
          >
            <Text className="text-label-sm text-foreground-tertiary mb-3">Amount</Text>
            <View className="bg-background-secondary rounded-xl p-4 border border-border">
              <View className="flex-row items-center">
                <TextInput
                  className="flex-1 text-display-sm text-foreground-primary"
                  placeholder="0.00"
                  placeholderTextColor={colors.foreground.tertiary}
                  value={amount}
                  onChangeText={setAmount}
                  keyboardType="decimal-pad"
                  autoFocus
                />
                {tokenName && (
                  <Text className="text-headline-sm text-foreground-secondary ml-2">{tokenName}</Text>
                )}
              </View>
              <View className="flex-row items-center justify-between mt-4 pt-4 border-t border-border">
                <Text className="text-body-sm text-foreground-tertiary">
                  Balance: {formatBalance(availableBalance)}
                </Text>
                <Pressable
                  className="px-3 py-1.5 rounded-lg"
                  style={{ backgroundColor: `${walletColors[selectedWallet]}20` }}
                  onPress={handleMaxPress}
                >
                  <Text
                    className="text-label-sm"
                    style={{ color: walletColors[selectedWallet] }}
                  >
                    MAX
                  </Text>
                </Pressable>
              </View>
            </View>
          </MotiView>
        </View>

        {/* Continue Button */}
        <View className="px-6 pb-6">
          <Pressable
            className={`py-4 rounded-xl flex-row items-center justify-center ${
              isValidAmount ? 'bg-accent active:opacity-80' : 'bg-background-tertiary opacity-50'
            }`}
            onPress={handleContinue}
            disabled={!isValidAmount}
          >
            <Text
              className={`text-label-lg ${
                isValidAmount ? 'text-foreground-primary' : 'text-foreground-disabled'
              }`}
            >
              Continue
            </Text>
            <ChevronRight
              size={20}
              color={isValidAmount ? colors.foreground.primary : colors.foreground.disabled}
              className="ml-1"
            />
          </Pressable>
        </View>
      </KeyboardAvoidingView>
    </SafeAreaView>
  );
}
