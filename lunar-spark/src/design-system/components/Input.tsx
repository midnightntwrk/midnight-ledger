import { useState } from 'react';
import { View, TextInput, Text, TextInputProps, Pressable } from 'react-native';
import { Eye, EyeOff } from 'lucide-react-native';
import { colors } from '../tokens';

interface InputProps extends Omit<TextInputProps, 'style'> {
  label?: string;
  error?: string;
  hint?: string;
  leftIcon?: React.ReactNode;
  rightIcon?: React.ReactNode;
  isPassword?: boolean;
}

export function Input({
  label,
  error,
  hint,
  leftIcon,
  rightIcon,
  isPassword,
  ...props
}: InputProps) {
  const [isFocused, setIsFocused] = useState(false);
  const [showPassword, setShowPassword] = useState(false);

  const hasError = !!error;
  const borderColor = hasError
    ? 'border-error'
    : isFocused
    ? 'border-accent'
    : 'border-border';

  return (
    <View className="mb-4">
      {label && (
        <Text className="text-label-sm text-foreground-tertiary mb-2">{label}</Text>
      )}
      <View
        className={`
          flex-row items-center
          bg-background-secondary
          rounded-xl
          border ${borderColor}
          px-4
        `}
      >
        {leftIcon && <View className="mr-3">{leftIcon}</View>}
        <TextInput
          className="flex-1 py-4 text-body-md text-foreground-primary"
          placeholderTextColor={colors.foreground.tertiary}
          onFocus={() => setIsFocused(true)}
          onBlur={() => setIsFocused(false)}
          secureTextEntry={isPassword && !showPassword}
          {...props}
        />
        {isPassword && (
          <Pressable onPress={() => setShowPassword(!showPassword)} className="ml-3 p-1">
            {showPassword ? (
              <EyeOff size={20} color={colors.foreground.tertiary} />
            ) : (
              <Eye size={20} color={colors.foreground.tertiary} />
            )}
          </Pressable>
        )}
        {rightIcon && !isPassword && <View className="ml-3">{rightIcon}</View>}
      </View>
      {(error || hint) && (
        <Text
          className={`text-body-sm mt-2 ${hasError ? 'text-error' : 'text-foreground-tertiary'}`}
        >
          {error || hint}
        </Text>
      )}
    </View>
  );
}
