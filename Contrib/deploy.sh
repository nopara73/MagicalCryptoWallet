set -e

SERVICE="magicalcryptowallet.service"

# Restarting MagicalCryptoWallet service....
sudo systemctl restart $SERVICE
echo "[OK] MagicalCryptoWallet service was restarted"

# Checking deployment...
sleep 1
systemctl status $SERVICE --no-pager
MAGICALCRYPTOWALLET_SERVICE_STATUS="$(systemctl is-active $SERVICE)"
if [ "${MAGICALCRYPTOWALLET_SERVICE_STATUS}" = "active" ]; then
   echo "$SERVICE is running"
else
   echo "$SERVICE is NOT running"
fi
