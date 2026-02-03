import { Pressable, Text, ActivityIndicator, View } from 'react-native';
import { MotiView } from 'moti';
import * as Haptics from 'expo-haptics';
import { colors } from '../tokens';

type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger';
type ButtonSize = 'sm' | 'md' | 'lg';

interface ButtonProps {
  children: string;
  variant?: ButtonVariant;
  size?: ButtonSize;
  disabled?: boolean;
  loading?: boolean;
  icon?: React.ReactNode;
  iconPosition?: 'left' | 'right';
  fullWidth?: boolean;
  onPress?: () => void;
}

const variantStyles: Record<ButtonVariant, { bg: string; bgPressed: string; text: string }> = {
  primary: {
    bg: 'bg-accent',
    bgPressed: 'bg-accent-hover',
    text: 'text-foreground-primary',
  },
  secondary: {
    bg: 'bg-background-secondary',
    bgPressed: 'bg-background-tertiary',
    text: 'text-foreground-primary',
  },
  ghost: {
    bg: 'bg-transparent',
    bgPressed: 'bg-background-secondary',
    text: 'text-accent',
  },
  danger: {
    bg: 'bg-error',
    bgPressed: 'bg-error-dark',
    text: 'text-foreground-primary',
  },
};

const sizeStyles: Record<ButtonSize, { padding: string; text: string; height: string }> = {
  sm: { padding: 'px-4 py-2', text: 'text-label-sm', height: 'h-9' },
  md: { padding: 'px-5 py-3', text: 'text-label-md', height: 'h-11' },
  lg: { padding: 'px-6 py-4', text: 'text-label-lg', height: 'h-14' },
};

export function Button({
  children,
  variant = 'primary',
  size = 'md',
  disabled = false,
  loading = false,
  icon,
  iconPosition = 'left',
  fullWidth = false,
  onPress,
}: ButtonProps) {
  const styles = variantStyles[variant];
  const sizes = sizeStyles[size];

  const handlePress = () => {
    if (!disabled && !loading) {
      Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light);
      onPress?.();
    }
  };

  const isDisabled = disabled || loading;

  return (
    <MotiView
      from={{ scale: 1 }}
      animate={{ scale: 1 }}
      transition={{ type: 'timing', duration: 100 }}
    >
      <Pressable
        className={`
          ${styles.bg}
          ${sizes.padding}
          ${sizes.height}
          ${fullWidth ? 'w-full' : ''}
          rounded-xl
          flex-row
          items-center
          justify-center
          ${isDisabled ? 'opacity-50' : 'active:opacity-80'}
        `}
        onPress={handlePress}
        disabled={isDisabled}
      >
        {loading ? (
          <ActivityIndicator
            size="small"
            color={variant === 'ghost' ? colors.accent.default : colors.foreground.primary}
          />
        ) : (
          <View className="flex-row items-center">
            {icon && iconPosition === 'left' && <View className="mr-2">{icon}</View>}
            <Text className={`${sizes.text} ${styles.text} font-semibold`}>{children}</Text>
            {icon && iconPosition === 'right' && <View className="ml-2">{icon}</View>}
          </View>
        )}
      </Pressable>
    </MotiView>
  );
}
