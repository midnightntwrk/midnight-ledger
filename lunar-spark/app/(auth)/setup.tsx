import { useState, useEffect } from 'react';
import { View, Text, TextInput, Pressable, ActivityIndicator, KeyboardAvoidingView, Platform } from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { useRouter } from 'expo-router';
import { MotiView } from 'moti';
import { ChevronLeft, Check } from 'lucide-react-native';
import { useWallet } from '../../src/providers';
import { ENVIRONMENT_OPTIONS, type Environment } from '../../src/config/environments';
import { colors } from '../../src/design-system';

export default function SetupScreen() {
  const router = useRouter();
  const { status, error, environment, setEnvironment, connect } = useWallet();
  const [seedHex, setSeedHex] = useState('');

  const isConnecting = status === 'connecting';
  const isValidSeed = seedHex.length === 64;

  // Navigate to tabs when connected
  useEffect(() => {
    if (status === 'connected') {
      router.replace('/(tabs)');
    }
  }, [status, router]);

  const handleConnect = async () => {
    await connect(seedHex);
  };

  if (isConnecting) {
    return (
      <SafeAreaView className="flex-1 bg-background-primary">
        <View className="flex-1 justify-center items-center px-6">
          <MotiView
            from={{ opacity: 0, scale: 0.9 }}
            animate={{ opacity: 1, scale: 1 }}
            transition={{ type: 'timing', duration: 400 }}
          >
            <ActivityIndicator size="large" color={colors.accent.default} />
            <Text className="text-headline-sm text-foreground-primary text-center mt-6">
              Initializing Wallet
            </Text>
            <Text className="text-body-md text-foreground-secondary text-center mt-2">
              Connecting to {environment}...
            </Text>
          </MotiView>
        </View>
      </SafeAreaView>
    );
  }

  return (
    <SafeAreaView className="flex-1 bg-background-primary">
      <KeyboardAvoidingView
        className="flex-1"
        behavior={Platform.OS === 'ios' ? 'padding' : 'height'}
      >
        {/* Header */}
        <View className="flex-row items-center px-4 py-3">
          <Pressable
            className="w-10 h-10 items-center justify-center rounded-full active:bg-background-secondary"
            onPress={() => router.back()}
          >
            <ChevronLeft size={24} color={colors.foreground.primary} />
          </Pressable>
        </View>

        <View className="flex-1 px-6">
          {/* Title */}
          <MotiView
            from={{ opacity: 0, translateY: 10 }}
            animate={{ opacity: 1, translateY: 0 }}
            transition={{ type: 'timing', duration: 400 }}
          >
            <Text className="text-headline-lg text-foreground-primary mb-2">
              Connect Wallet
            </Text>
            <Text className="text-body-md text-foreground-secondary mb-8">
              Select your network and enter your seed phrase
            </Text>
          </MotiView>

          {/* Network Selection */}
          <MotiView
            from={{ opacity: 0, translateY: 10 }}
            animate={{ opacity: 1, translateY: 0 }}
            transition={{ type: 'timing', duration: 400, delay: 100 }}
            className="mb-6"
          >
            <Text className="text-label-md text-accent mb-3">Network</Text>
            <View className="flex-row flex-wrap gap-2">
              {ENVIRONMENT_OPTIONS.map((opt) => (
                <Pressable
                  key={opt.value}
                  className={`px-4 py-2.5 rounded-lg border ${
                    environment === opt.value
                      ? 'bg-accent/20 border-accent'
                      : 'bg-background-secondary border-border'
                  }`}
                  onPress={() => setEnvironment(opt.value as Environment)}
                >
                  <Text
                    className={`text-label-md ${
                      environment === opt.value
                        ? 'text-foreground-primary'
                        : 'text-foreground-secondary'
                    }`}
                  >
                    {opt.label}
                  </Text>
                </Pressable>
              ))}
            </View>
          </MotiView>

          {/* Seed Input */}
          <MotiView
            from={{ opacity: 0, translateY: 10 }}
            animate={{ opacity: 1, translateY: 0 }}
            transition={{ type: 'timing', duration: 400, delay: 200 }}
            className="mb-6"
          >
            <Text className="text-label-md text-accent mb-3">Wallet Seed (Hex)</Text>
            <TextInput
              className="bg-background-secondary border border-border rounded-xl p-4 text-foreground-primary font-mono text-body-sm min-h-[100px]"
              placeholder="Enter 64 character hex seed..."
              placeholderTextColor={colors.foreground.tertiary}
              value={seedHex}
              onChangeText={setSeedHex}
              autoCapitalize="none"
              autoCorrect={false}
              multiline
              textAlignVertical="top"
            />
            <View className="flex-row justify-between items-center mt-2">
              <Text className="text-body-sm text-foreground-tertiary">
                {seedHex.length}/64 characters
              </Text>
              {isValidSeed && (
                <View className="flex-row items-center">
                  <Check size={14} color={colors.success.default} />
                  <Text className="text-body-sm text-success ml-1">Valid</Text>
                </View>
              )}
            </View>
          </MotiView>

          {/* Error Display */}
          {error && (
            <MotiView
              from={{ opacity: 0, scale: 0.95 }}
              animate={{ opacity: 1, scale: 1 }}
              className="bg-error/10 border border-error/30 rounded-xl p-4 mb-6"
            >
              <Text className="text-body-sm text-error">{error}</Text>
            </MotiView>
          )}
        </View>

        {/* Connect Button */}
        <View className="px-6 pb-6">
          <Pressable
            className={`py-4 rounded-xl items-center ${
              isValidSeed ? 'bg-accent active:opacity-80' : 'bg-background-tertiary opacity-50'
            }`}
            onPress={handleConnect}
            disabled={!isValidSeed}
          >
            <Text
              className={`text-label-lg ${
                isValidSeed ? 'text-foreground-primary' : 'text-foreground-disabled'
              }`}
            >
              Connect Wallet
            </Text>
          </Pressable>
        </View>
      </KeyboardAvoidingView>
    </SafeAreaView>
  );
}
