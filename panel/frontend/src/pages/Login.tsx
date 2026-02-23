import { useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  Box,
  Button,
  Container,
  FormControl,
  FormLabel,
  Heading,
  Input,
  useToast,
  VStack,
} from "@chakra-ui/react";
import { useLogin } from "../api/auth";

export default function Login() {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const navigate = useNavigate();
  const toast = useToast();
  const login = useLogin();

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    login.mutate(
      { username, password },
      {
        onSuccess: () => {
          toast({ title: "Logged in", status: "success" });
          navigate("/", { replace: true });
        },
        onError: (err: Error) => {
          toast({ title: err.message, status: "error" });
        },
      }
    );
  }

  return (
    <Container maxW="md" py={20}>
      <Box p={8} shadow="md" borderWidth="1px" borderRadius="lg">
        <Heading size="lg" mb={6} textAlign="center">
          MTProxy Panel
        </Heading>
        <form onSubmit={handleSubmit}>
          <VStack gap={4}>
            <FormControl isRequired>
              <FormLabel>Username</FormLabel>
              <Input
                autoComplete="username"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
              />
            </FormControl>
            <FormControl isRequired>
              <FormLabel>Password</FormLabel>
              <Input
                type="password"
                autoComplete="current-password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
            </FormControl>
            <Button
              type="submit"
              width="full"
              colorScheme="blue"
              isLoading={login.isPending}
            >
              Sign in
            </Button>
          </VStack>
        </form>
      </Box>
    </Container>
  );
}
