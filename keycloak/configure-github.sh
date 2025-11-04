#!/bin/bash

# Script to configure GitHub OAuth credentials in Keycloak realm export
# Usage: ./configure-github.sh <client_id> <client_secret>

if [ "$#" -ne 2 ]; then
    echo "Usage: $0 <github_client_id> <github_client_secret>"
    echo "Example: $0 Iv1.abc123def456 1234567890abcdef1234567890abcdef12345678"
    exit 1
fi

CLIENT_ID="$1"
CLIENT_SECRET="$2"

# Backup the original file
cp oauth2-realm.json oauth2-realm.json.bak

# Replace placeholders with actual credentials
sed -i.tmp \
    -e "s/GITHUB_CLIENT_ID_PLACEHOLDER/$CLIENT_ID/g" \
    -e "s/GITHUB_CLIENT_SECRET_PLACEHOLDER/$CLIENT_SECRET/g" \
    oauth2-realm.json

# Remove the temporary file created by sed on macOS
rm -f oauth2-realm.json.tmp

echo "✅ GitHub OAuth credentials configured!"
echo "   Client ID: ${CLIENT_ID:0:20}..."
echo "   Client Secret: ${CLIENT_SECRET:0:10}..."
echo ""
echo "Backup saved to: oauth2-realm.json.bak"
echo ""
echo "Next steps:"
echo "1. Run 'docker compose up -d' to start services"
echo "2. Wait for Keycloak to start (check with 'docker compose logs -f keycloak')"
echo "3. Access Keycloak at http://localhost:8080"
echo "   - Admin username: admin"
echo "   - Admin password: admin"
echo "4. Test user credentials:"
echo "   - Username: testuser"
echo "   - Password: testpass"
