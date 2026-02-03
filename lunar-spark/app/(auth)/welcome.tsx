import { View, Text, Pressable } from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { useRouter } from 'expo-router';
import { MotiView } from 'moti';
import { Wallet } from 'lucide-react-native';
import { colors } from '../../src/design-system';

export default function WelcomeScreen() {
  const router = useRouter();

  return (
    <SafeAreaView className="flex-1 bg-background-primary">
      <View className="flex-1 justify-center items-center px-6">
        {/* Logo/Icon */}
        <MotiView
          from={{ opacity: 0, scale: 0.8 }}
          animate={{ opacity: 1, scale: 1 }}
          transition={{ type: 'timing', duration: 600 }}
          className="mb-8"
        >
          <View className="w-24 h-24 rounded-3xl bg-accent/20 items-center justify-center">
            <Wallet size={48} color={colors.accent.default} strokeWidth={1.5} />
          </View>
        </MotiView>

        {/* Title */}
        <MotiView
          from={{ opacity: 0, translateY: 20 }}
          animate={{ opacity: 1, translateY: 0 }}
          transition={{ type: 'timing', duration: 600, delay: 200 }}
        >
          <Text className="text-display-sm text-foreground-primary text-center mb-3">
            Lunar Spark
          </Text>
          <Text className="text-body-lg text-foreground-secondary text-center mb-12">
            Your gateway to Midnight
          </Text>
        </MotiView>

        {/* CTA */}
        <MotiView
          from={{ opacity: 0, translateY: 20 }}
          animate={{ opacity: 1, translateY: 0 }}
          transition={{ type: 'timing', duration: 600, delay: 400 }}
          className="w-full"
        >
          <Pressable
            className="bg-accent py-4 rounded-xl items-center active:opacity-80"
            onPress={() => router.push('/(auth)/setup')}
          >
            <Text className="text-label-lg text-foreground-primary">Get Started</Text>
          </Pressable>
        </MotiView>
      </View>

      {/* Footer */}
      <View className="pb-8 px-6">
        <Text className="text-body-sm text-foreground-tertiary text-center">
          By continuing, you agree to our Terms of Service
        </Text>
      </View>
    </SafeAreaView>
  );
}
