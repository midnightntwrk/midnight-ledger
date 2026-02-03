import { View, Text, Pressable, Alert } from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { useRouter } from 'expo-router';
import { MotiView } from 'moti';
import {
  Globe,
  Moon,
  Sun,
  ChevronRight,
  LogOut,
  Shield,
  Info,
} from 'lucide-react-native';
import { useWallet } from '../../src/providers';
import { useTheme, colors } from '../../src/design-system';

interface SettingsItemProps {
  icon: React.ReactNode;
  label: string;
  value?: string;
  onPress?: () => void;
  danger?: boolean;
}

function SettingsItem({ icon, label, value, onPress, danger }: SettingsItemProps) {
  return (
    <Pressable
      className="flex-row items-center py-4 active:opacity-60"
      onPress={onPress}
      disabled={!onPress}
    >
      <View className="w-10 h-10 rounded-full bg-background-tertiary items-center justify-center mr-4">
        {icon}
      </View>
      <View className="flex-1">
        <Text
          className={`text-label-lg ${danger ? 'text-error' : 'text-foreground-primary'}`}
        >
          {label}
        </Text>
      </View>
      {value && (
        <Text className="text-body-md text-foreground-tertiary mr-2">{value}</Text>
      )}
      {onPress && <ChevronRight size={20} color={colors.foreground.tertiary} />}
    </Pressable>
  );
}

function SettingsSection({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <View className="mb-6">
      <Text className="text-label-sm text-foreground-tertiary uppercase tracking-wider mb-2 px-4">
        {title}
      </Text>
      <View className="bg-background-secondary rounded-2xl px-4 divide-y divide-border">
        {children}
      </View>
    </View>
  );
}

export default function SettingsScreen() {
  const router = useRouter();
  const { environment, disconnect } = useWallet();
  const { colorScheme, preference, setPreference } = useTheme();

  const handleDisconnect = () => {
    Alert.alert(
      'Disconnect Wallet',
      'Are you sure you want to disconnect your wallet? You will need to re-enter your seed to reconnect.',
      [
        { text: 'Cancel', style: 'cancel' },
        {
          text: 'Disconnect',
          style: 'destructive',
          onPress: async () => {
            await disconnect();
            router.replace('/(auth)/welcome');
          },
        },
      ]
    );
  };

  const toggleTheme = () => {
    const next = preference === 'dark' ? 'light' : preference === 'light' ? 'system' : 'dark';
    setPreference(next);
  };

  const themeLabel =
    preference === 'system' ? 'System' : preference === 'dark' ? 'Dark' : 'Light';

  return (
    <SafeAreaView className="flex-1 bg-background-primary" edges={['top']}>
      {/* Header */}
      <View className="px-6 pt-4 pb-6">
        <Text className="text-headline-lg text-foreground-primary">Settings</Text>
      </View>

      <MotiView
        from={{ opacity: 0, translateY: 20 }}
        animate={{ opacity: 1, translateY: 0 }}
        transition={{ type: 'timing', duration: 400 }}
        className="px-6"
      >
        {/* Network */}
        <SettingsSection title="Network">
          <SettingsItem
            icon={<Globe size={20} color={colors.accent.default} />}
            label="Network"
            value={environment.toUpperCase()}
          />
        </SettingsSection>

        {/* Appearance */}
        <SettingsSection title="Appearance">
          <SettingsItem
            icon={
              colorScheme === 'dark' ? (
                <Moon size={20} color={colors.accent.default} />
              ) : (
                <Sun size={20} color={colors.accent.default} />
              )
            }
            label="Theme"
            value={themeLabel}
            onPress={toggleTheme}
          />
        </SettingsSection>

        {/* Security */}
        <SettingsSection title="Security">
          <SettingsItem
            icon={<Shield size={20} color={colors.accent.default} />}
            label="Biometric Authentication"
            value="Coming Soon"
          />
        </SettingsSection>

        {/* About */}
        <SettingsSection title="About">
          <SettingsItem
            icon={<Info size={20} color={colors.accent.default} />}
            label="Version"
            value="1.0.0"
          />
        </SettingsSection>

        {/* Disconnect */}
        <SettingsSection title="Account">
          <SettingsItem
            icon={<LogOut size={20} color={colors.error.default} />}
            label="Disconnect Wallet"
            onPress={handleDisconnect}
            danger
          />
        </SettingsSection>
      </MotiView>
    </SafeAreaView>
  );
}
