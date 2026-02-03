import { View, Text } from 'react-native';

type BadgeVariant = 'default' | 'success' | 'warning' | 'error' | 'info';
type BadgeSize = 'sm' | 'md';

interface BadgeProps {
  children: string;
  variant?: BadgeVariant;
  size?: BadgeSize;
  dot?: boolean;
}

const variantStyles: Record<BadgeVariant, { bg: string; text: string; dot: string }> = {
  default: {
    bg: 'bg-background-tertiary',
    text: 'text-foreground-secondary',
    dot: 'bg-foreground-secondary',
  },
  success: {
    bg: 'bg-success/20',
    text: 'text-success',
    dot: 'bg-success',
  },
  warning: {
    bg: 'bg-warning/20',
    text: 'text-warning',
    dot: 'bg-warning',
  },
  error: {
    bg: 'bg-error/20',
    text: 'text-error',
    dot: 'bg-error',
  },
  info: {
    bg: 'bg-info/20',
    text: 'text-info',
    dot: 'bg-info',
  },
};

const sizeStyles: Record<BadgeSize, { padding: string; text: string; dot: string }> = {
  sm: { padding: 'px-2 py-0.5', text: 'text-label-sm', dot: 'w-1.5 h-1.5' },
  md: { padding: 'px-3 py-1', text: 'text-label-md', dot: 'w-2 h-2' },
};

export function Badge({
  children,
  variant = 'default',
  size = 'md',
  dot = false,
}: BadgeProps) {
  const styles = variantStyles[variant];
  const sizes = sizeStyles[size];

  return (
    <View className={`flex-row items-center ${styles.bg} ${sizes.padding} rounded-full`}>
      {dot && <View className={`${sizes.dot} rounded-full ${styles.dot} mr-1.5`} />}
      <Text className={`${sizes.text} ${styles.text} font-semibold`}>{children}</Text>
    </View>
  );
}
