RED="\033[31m"
YELLOW="\033[33m"
GREEN="\033[32m"
RESET="\033[0m"

# Install dependencies
os_type=$(uname)
case "$os_type" in
    "Darwin")
        echo "${YELLOW}=> Installing dependencies for MacOS...${RESET}"
#        brew install automake libtool boost pkg-config libevent miniupnpc
        ;;
    "Linux")
        echo "${YELLOW}=> Installing dependencies for Linux...${RESET}"
        sudo apt install -y build-essential libtool autotools-dev automake pkg-config bsdmainutils curl git libboost-all-dev sqlite3 libsqlite3-dev
        ;;
    *)
        echo "${RED}Unsupported OS: $os_type ${RESET}"
        ;;
esac

# Clone repo
echo "${YELLOW}=> Cloning modified bitcoind repo...${RESET}"
git clone https://github.com/Sethu98/bitcoin.git
cd bitcoin || exit
git checkout mod_27_v2

# Build
echo "${YELLOW}=> Building bitcoin core...${RESET}"
./autogen.sh
./configure
make -j16

# Install
echo "${YELLOW}=> Installing bitcoin core...${RESET}"
sudo make install

echo "${GREEN}=> Bitcoin core setup complete!${RESET}"

## Delete the repo ?
#cd .. || exit
#rm -rf bitcoin
