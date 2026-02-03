import { View, Pressable, ViewProps } from 'react-native';
import { MotiView } from 'moti';

type CardVariant = 'default' | 'elevated' | 'outlined';

interface CardProps extends ViewProps {
  children: React.ReactNode;
  variant?: CardVariant;
  onPress?: () => void;
  animated?: boolean;
}

const variantStyles: Record<CardVariant, string> = {
  default: 'bg-background-secondary border border-border',
  elevated: 'bg-background-elevated shadow-lg',
  outlined: 'bg-transparent border border-border',
};

export function Card({
  children,
  variant = 'default',
  onPress,
  animated = true,
  className = '',
  ...props
}: CardProps) {
  const baseStyles = `${variantStyles[variant]} rounded-2xl overflow-hidden`;
  const combinedStyles = `${baseStyles} ${className}`;

  const content = (
    <View className={combinedStyles} {...props}>
      {children}
    </View>
  );

  if (onPress) {
    return (
      <Pressable onPress={onPress} className="active:opacity-80">
        {animated ? (
          <MotiView
            from={{ opacity: 0, translateY: 10 }}
            animate={{ opacity: 1, translateY: 0 }}
            transition={{ type: 'timing', duration: 300 }}
          >
            {content}
          </MotiView>
        ) : (
          content
        )}
      </Pressable>
    );
  }

  if (animated) {
    return (
      <MotiView
        from={{ opacity: 0, translateY: 10 }}
        animate={{ opacity: 1, translateY: 0 }}
        transition={{ type: 'timing', duration: 300 }}
      >
        {content}
      </MotiView>
    );
  }

  return content;
}
