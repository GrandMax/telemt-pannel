import { Routes, Route, NavLink } from "react-router-dom";
import {
  Box,
  Container,
  Heading,
  HStack,
  Button,
  Spacer,
  Link,
} from "@chakra-ui/react";
import { useAuthStore } from "../stores/authStore";
import { useMe } from "../api/auth";
import Users from "./Users";
import DashboardCharts from "../components/DashboardCharts";

function DashboardShell({ children }: { children: React.ReactNode }) {
  const { data: me } = useMe();
  const setToken = useAuthStore((s) => s.setToken);

  return (
    <Box minH="100vh" bg="gray.50">
      <Box as="header" bg="white" shadow="sm" py={3} px={4}>
        <Container maxW="6xl">
          <HStack gap={6}>
            <Heading size="md">MTProxy Panel</Heading>
            <Link as={NavLink} to="/" fontWeight="medium">
              Dashboard
            </Link>
            <Link as={NavLink} to="/users" fontWeight="medium">
              Users
            </Link>
            <Spacer />
            {me && (
              <span style={{ color: "var(--chakra-colors-gray-600)" }}>
                {me.username}
              </span>
            )}
            <Button size="sm" variant="outline" onClick={() => setToken(null)}>
              Logout
            </Button>
          </HStack>
        </Container>
      </Box>
      <Container maxW="6xl" py={6}>
        {children}
      </Container>
    </Box>
  );
}

export default function DashboardLayout() {
  return (
    <DashboardShell>
      <Routes>
        <Route path="/" element={<DashboardHome />} />
        <Route path="/users" element={<Users />} />
      </Routes>
    </DashboardShell>
  );
}


function DashboardHome() {
  return (
    <Box>
      <Heading size="lg" mb={4}>
        Dashboard
      </Heading>
      <DashboardCharts />
    </Box>
  );
}
