import { useState } from 'react';
import { View, Text, Pressable, ActivityIndicator, Alert } from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { useRouter, useLocalSearchParams } from 'expo-router';
import { MotiView } from 'moti';
import { ChevronLeft, AlertCircle } from 'lucide-react-native';
import * as Haptics from 'expo-haptics';
import { colors } from '../../../src/design-system';
import { formatBalance } from '../../../src/utils/balance';
import { useWallet, type TransferWalletType } from '../../../src/providers';

export default function ConfirmScreen() {
  const router = useRouter();
  const { transfer } = useWallet();
  const params = useLocalSearchParams<{ wallet: string; amount: string; recipient: string }>();
  const [isSubmitting, setIsSubmitting] = useState(false);

  const amount = params.amount ? BigInt(params.amount) : 0n;
  const wallet = (params.wallet ?? 'shielded') as TransferWalletType;
  const recipient = params.recipient ?? '';
  const tokenName = wallet === 'unshielded' ? 'tNIGHT' : '';

  const truncatedRecipient = recipient
    ? `${recipient.slice(0, 16)}...${recipient.slice(-12)}`
    : '';

  const handleConfirm = async () => {
    setIsSubmitting(true);
    Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Medium);

    try {
      await transfer(wallet, recipient, amount);

      // Navigate to success screen
      router.replace({
        pathname: '/(modals)/send/success',
        params: {
          amount: params.amount,
          recipient,
          wallet,
        },
      });
    } catch (err) {
      console.error('[ConfirmScreen] Transfer failed:', err);
      Haptics.notificationAsync(Haptics.NotificationFeedbackType.Error);
      Alert.alert(
        'Transfer Failed',
        err instanceof Error ? err.message : 'An unknown error occurred',
        [{ text: 'OK' }]
      );
      setIsSubmitting(false);
    }
  };

  return (
    <SafeAreaView className="flex-1 bg-background-primary">
      {/* Header */}
      <View className="flex-row items-center px-4 py-4">
        <Pressable
          className="w-10 h-10 rounded-full items-center justify-center active:opacity-60"
          onPress={() => router.back()}
          disabled={isSubmitting}
        >
          <ChevronLeft size={24} color={colors.foreground.primary} />
        </Pressable>
        <Text className="flex-1 text-headline-md text-foreground-primary text-center mr-10">
          Confirm
        </Text>
      </View>

      <View className="flex-1 px-6">
        {/* Transaction Summary */}
        <MotiView
          from={{ opacity: 0, translateY: 20 }}
          animate={{ opacity: 1, translateY: 0 }}
          transition={{ type: 'timing', duration: 400 }}
          className="items-center mb-8"
        >
          <Text className="text-body-md text-foreground-tertiary mb-2">You are sending</Text>
          <Text className="text-display-md text-foreground-primary">
            {formatBalance(amount)}
          </Text>
          {tokenName && (
            <Text className="text-headline-sm text-foreground-secondary">{tokenName}</Text>
          )}
        </MotiView>

        {/* Details Card */}
        <MotiView
          from={{ opacity: 0, translateY: 20 }}
          animate={{ opacity: 1, translateY: 0 }}
          transition={{ type: 'timing', duration: 400, delay: 100 }}
          className="bg-background-secondary rounded-2xl border border-border overflow-hidden"
        >
          <View className="p-4 border-b border-border">
            <Text className="text-label-sm text-foreground-tertiary mb-1">From</Text>
            <Text className="text-body-md text-foreground-primary capitalize">{wallet} Wallet</Text>
          </View>
          <View className="p-4 border-b border-border">
            <Text className="text-label-sm text-foreground-tertiary mb-1">To</Text>
            <Text className="text-body-sm text-foreground-primary font-mono">
              {truncatedRecipient}
            </Text>
          </View>
          <View className="p-4">
            <Text className="text-label-sm text-foreground-tertiary mb-1">Network Fee</Text>
            <Text className="text-body-md text-foreground-primary">~0.001 tDUST</Text>
          </View>
        </MotiView>

        {/* Warning */}
        <MotiView
          from={{ opacity: 0, translateY: 20 }}
          animate={{ opacity: 1, translateY: 0 }}
          transition={{ type: 'timing', duration: 400, delay: 200 }}
          className="flex-row items-start bg-warning/10 rounded-xl p-4 mt-6"
        >
          <AlertCircle size={20} color={colors.warning.default} className="mt-0.5" />
          <Text className="flex-1 text-body-sm text-warning ml-3">
            Please verify the recipient address. Transactions cannot be reversed once confirmed.
          </Text>
        </MotiView>
      </View>

      {/* Confirm Button */}
      <View className="px-6 pb-6">
        <Pressable
          className={`py-4 rounded-xl items-center justify-center ${
            isSubmitting ? 'bg-accent/50' : 'bg-accent active:opacity-80'
          }`}
          onPress={handleConfirm}
          disabled={isSubmitting}
        >
          {isSubmitting ? (
            <View className="flex-row items-center">
              <ActivityIndicator size="small" color={colors.foreground.primary} />
              <Text className="text-label-lg text-foreground-primary ml-2">
                Submitting...
              </Text>
            </View>
          ) : (
            <Text className="text-label-lg text-foreground-primary">Confirm & Send</Text>
          )}
        </Pressable>
      </View>
    </SafeAreaView>
  );
}
