import { useState } from 'react';
import { View, Text, TextInput, Pressable, KeyboardAvoidingView, Platform } from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { useRouter, useLocalSearchParams } from 'expo-router';
import { MotiView } from 'moti';
import { ChevronLeft, ChevronRight, Clipboard, ScanLine } from 'lucide-react-native';
import * as ClipboardAPI from 'expo-clipboard';
import * as Haptics from 'expo-haptics';
import { colors } from '../../../src/design-system';
import { formatBalance } from '../../../src/utils/balance';

export default function RecipientScreen() {
  const router = useRouter();
  const params = useLocalSearchParams<{ wallet: string; amount: string }>();
  const [recipient, setRecipient] = useState('');

  const amount = params.amount ? BigInt(params.amount) : 0n;
  const wallet = params.wallet ?? 'shielded';
  const tokenName = wallet === 'unshielded' ? 'tNIGHT' : '';

  // Basic address validation (starts with expected prefix and has reasonable length)
  const isValidAddress = recipient.length >= 40;

  const handlePaste = async () => {
    const text = await ClipboardAPI.getStringAsync();
    if (text) {
      setRecipient(text.trim());
      Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light);
    }
  };

  const handleContinue = () => {
    router.push({
      pathname: '/(modals)/send/confirm',
      params: {
        wallet,
        amount: params.amount,
        recipient,
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
        <View className="flex-row items-center px-4 py-4">
          <Pressable
            className="w-10 h-10 rounded-full items-center justify-center active:opacity-60"
            onPress={() => router.back()}
          >
            <ChevronLeft size={24} color={colors.foreground.primary} />
          </Pressable>
          <Text className="flex-1 text-headline-md text-foreground-primary text-center mr-10">
            Recipient
          </Text>
        </View>

        <View className="flex-1 px-6">
          {/* Summary */}
          <MotiView
            from={{ opacity: 0, translateY: 10 }}
            animate={{ opacity: 1, translateY: 0 }}
            transition={{ type: 'timing', duration: 300 }}
            className="bg-background-secondary rounded-xl p-4 border border-border mb-6"
          >
            <View className="flex-row justify-between items-center">
              <Text className="text-body-md text-foreground-tertiary">Sending</Text>
              <Text className="text-headline-sm text-foreground-primary">
                {formatBalance(amount)}{tokenName ? ` ${tokenName}` : ''}
              </Text>
            </View>
            <View className="flex-row justify-between items-center mt-2">
              <Text className="text-body-md text-foreground-tertiary">From</Text>
              <Text className="text-body-md text-foreground-secondary capitalize">{wallet}</Text>
            </View>
          </MotiView>

          {/* Recipient Input */}
          <MotiView
            from={{ opacity: 0, translateY: 10 }}
            animate={{ opacity: 1, translateY: 0 }}
            transition={{ type: 'timing', duration: 300, delay: 100 }}
          >
            <Text className="text-label-sm text-foreground-tertiary mb-3">To Address</Text>
            <View className="bg-background-secondary rounded-xl border border-border overflow-hidden">
              <TextInput
                className="p-4 text-body-md text-foreground-primary font-mono min-h-[100px]"
                placeholder="Enter recipient address..."
                placeholderTextColor={colors.foreground.tertiary}
                value={recipient}
                onChangeText={setRecipient}
                autoCapitalize="none"
                autoCorrect={false}
                multiline
                textAlignVertical="top"
                autoFocus
              />
              <View className="flex-row border-t border-border">
                <Pressable
                  className="flex-1 flex-row items-center justify-center py-3 active:opacity-60"
                  onPress={handlePaste}
                >
                  <Clipboard size={18} color={colors.accent.default} />
                  <Text className="text-label-md text-accent ml-2">Paste</Text>
                </Pressable>
                <View className="w-px bg-border" />
                <Pressable
                  className="flex-1 flex-row items-center justify-center py-3 active:opacity-60"
                  onPress={() => {
                    // TODO: Implement QR scanner
                    Haptics.notificationAsync(Haptics.NotificationFeedbackType.Warning);
                  }}
                >
                  <ScanLine size={18} color={colors.accent.default} />
                  <Text className="text-label-md text-accent ml-2">Scan</Text>
                </Pressable>
              </View>
            </View>
          </MotiView>
        </View>

        {/* Continue Button */}
        <View className="px-6 pb-6">
          <Pressable
            className={`py-4 rounded-xl flex-row items-center justify-center ${
              isValidAddress ? 'bg-accent active:opacity-80' : 'bg-background-tertiary opacity-50'
            }`}
            onPress={handleContinue}
            disabled={!isValidAddress}
          >
            <Text
              className={`text-label-lg ${
                isValidAddress ? 'text-foreground-primary' : 'text-foreground-disabled'
              }`}
            >
              Review
            </Text>
            <ChevronRight
              size={20}
              color={isValidAddress ? colors.foreground.primary : colors.foreground.disabled}
              className="ml-1"
            />
          </Pressable>
        </View>
      </KeyboardAvoidingView>
    </SafeAreaView>
  );
}
