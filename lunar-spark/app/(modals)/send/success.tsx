import { View, Text, Pressable } from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { useRouter, useLocalSearchParams } from 'expo-router';
import { MotiView } from 'moti';
import { CheckCircle, Home } from 'lucide-react-native';
import * as Haptics from 'expo-haptics';
import { useEffect } from 'react';
import { colors } from '../../../src/design-system';
import { formatBalance } from '../../../src/utils/balance';

export default function SuccessScreen() {
  const router = useRouter();
  const params = useLocalSearchParams<{ amount: string; recipient: string; wallet: string }>();

  const amount = params.amount ? BigInt(params.amount) : 0n;
  const recipient = params.recipient ?? '';
  const wallet = params.wallet ?? 'shielded';
  const tokenName = wallet === 'unshielded' ? 'tNIGHT' : '';
  const truncatedRecipient = recipient
    ? `${recipient.slice(0, 12)}...${recipient.slice(-8)}`
    : '';

  useEffect(() => {
    Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
  }, []);

  const handleDone = () => {
    // Navigate back to home, clearing the modal stack
    router.dismissAll();
    router.replace('/(tabs)');
  };

  return (
    <SafeAreaView className="flex-1 bg-background-primary">
      <View className="flex-1 justify-center items-center px-6">
        {/* Success Icon */}
        <MotiView
          from={{ opacity: 0, scale: 0.5 }}
          animate={{ opacity: 1, scale: 1 }}
          transition={{ type: 'spring', damping: 15 }}
          className="mb-8"
        >
          <View className="w-24 h-24 rounded-full bg-success/20 items-center justify-center">
            <CheckCircle size={56} color={colors.success.default} strokeWidth={1.5} />
          </View>
        </MotiView>

        {/* Title */}
        <MotiView
          from={{ opacity: 0, translateY: 20 }}
          animate={{ opacity: 1, translateY: 0 }}
          transition={{ type: 'timing', duration: 400, delay: 200 }}
        >
          <Text className="text-headline-lg text-foreground-primary text-center mb-2">
            Transaction Submitted
          </Text>
          <Text className="text-body-md text-foreground-secondary text-center">
            Your transaction is being processed
          </Text>
        </MotiView>

        {/* Amount */}
        <MotiView
          from={{ opacity: 0, translateY: 20 }}
          animate={{ opacity: 1, translateY: 0 }}
          transition={{ type: 'timing', duration: 400, delay: 300 }}
          className="mt-8 bg-background-secondary rounded-2xl p-6 w-full border border-border"
        >
          <View className="items-center mb-4">
            <Text className="text-body-sm text-foreground-tertiary mb-1">Amount Sent</Text>
            <Text className="text-display-sm text-foreground-primary">
              {formatBalance(amount)}
            </Text>
            {tokenName && (
              <Text className="text-body-md text-foreground-secondary">{tokenName}</Text>
            )}
          </View>
          <View className="border-t border-border pt-4">
            <Text className="text-body-sm text-foreground-tertiary text-center mb-1">To</Text>
            <Text className="text-body-sm text-foreground-secondary font-mono text-center">
              {truncatedRecipient}
            </Text>
          </View>
        </MotiView>

        {/* Info */}
        <MotiView
          from={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ type: 'timing', duration: 400, delay: 400 }}
          className="mt-6"
        >
          <Text className="text-body-sm text-foreground-tertiary text-center">
            You can view the transaction status in the Activity tab
          </Text>
        </MotiView>
      </View>

      {/* Done Button */}
      <View className="px-6 pb-6">
        <Pressable
          className="py-4 rounded-xl flex-row items-center justify-center bg-accent active:opacity-80"
          onPress={handleDone}
        >
          <Home size={20} color={colors.foreground.primary} />
          <Text className="text-label-lg text-foreground-primary ml-2">Back to Home</Text>
        </Pressable>
      </View>
    </SafeAreaView>
  );
}
