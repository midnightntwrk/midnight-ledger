import { View, Text } from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { MotiView } from 'moti';
import { Clock } from 'lucide-react-native';
import { colors } from '../../src/design-system';

export default function ActivityScreen() {
  return (
    <SafeAreaView className="flex-1 bg-background-primary" edges={['top']}>
      {/* Header */}
      <View className="px-6 pt-4 pb-6">
        <Text className="text-headline-lg text-foreground-primary">Activity</Text>
        <Text className="text-body-md text-foreground-secondary mt-1">
          Your transaction history
        </Text>
      </View>

      {/* Empty State */}
      <View className="flex-1 justify-center items-center px-6">
        <MotiView
          from={{ opacity: 0, scale: 0.9 }}
          animate={{ opacity: 1, scale: 1 }}
          transition={{ type: 'timing', duration: 500 }}
          className="items-center"
        >
          <View className="w-20 h-20 rounded-full bg-background-secondary items-center justify-center mb-6">
            <Clock size={40} color={colors.foreground.tertiary} strokeWidth={1.5} />
          </View>
          <Text className="text-headline-sm text-foreground-primary text-center mb-2">
            No Activity Yet
          </Text>
          <Text className="text-body-md text-foreground-secondary text-center max-w-[280px]">
            Your transaction history will appear here once you start sending and receiving tokens.
          </Text>
        </MotiView>
      </View>
    </SafeAreaView>
  );
}
