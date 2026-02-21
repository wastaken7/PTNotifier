# PTNotifier (Private Tracker Notifier)

PTNotifier is a Python-based tool designed to monitor private torrent trackers for new notifications and private messages, sending alerts to a specified Telegram and/or Discord chat.

## How It Works

The script dynamically loads tracker modules from the `trackers/` directory. For each tracker, it uses stored browser cookies to authenticate and then scrapes the notifications and messages pages for unread items. It maintains a simple state file for each tracker to keep track of processed items, ensuring that notifications are sent only once.

New notifications are formatted and sent to a Telegram chat or a Discord channel via a bot or a webhook. The script runs in a continuous loop, with a configurable interval between checks.

## Features

-   Monitors multiple private trackers simultaneously.
-   Sends notifications for new site alerts and private messages.
-   Easy to configure via a `config.py` file.
-   Uses Telegram and/or Discord for notifications.
-   Dynamically loads tracker modules.

## Supported Trackers

Trackers are managed in three categories based on how their cookies are loaded.

### AvistaZ & UNIT3D Trackers
These trackers share a common platform.
-   **AvistaZ**: For sites in the AvistaZ network (e.g., AvistaZ, PrivateHD, ExoticaZ). Place cookies in the `cookies/AvistaZ/` folder.
-   **UNIT3D**: For trackers using the UNIT3D framework. Place cookies in the `cookies/UNIT3D/` folder.

For these types, the name of the cookie file does not matter, as long as it is a `.txt` file.

### Other Trackers
These are specific trackers that have their own dedicated module. The cookie file **must be placed in the `cookies/Other/` directory** and **must have a specific name** that matches the tracker's module.

### Supported Trackers List
| Module | Website URL | Required Cookie Filename | Observation |
| :--- | :--- | :--- | :--- |
| `UNIT3D.py` | - | - | All UNIT3D trackers (in theory), if the tracker is highly customized, it is likely that it will not work. If you do not know whether a tracker is UNIT3D, check it's code base [here](https://hdvinnie.github.io/Private-Trackers-Spreadsheet/). |
| `AvistaZ.py` | - | - | This includes AvistaZ, CinemaZ, PrivateHD, ExoticaZ.|
| `AmigosShareClub.py` | `cliente.amigos-share.club`| `AmigosShareClub.txt` | - |
| `Anthelion.py` | `anthelion.me` | `Anthelion.txt` | - |
| `BJShare.py` | `bj-share.info`| `BJShare.txt` | - |
| `BrasilTracker.py` | `brasiltracker.org` | `BrasilTracker.txt` | - |
| `BTSCHOOL.py` | `pt.btschool.club`| `BTSCHOOL.txt` | - |
| `DigitalCore.py` | `digitalcore.club`| `DigitalCore.txt` | - |
| `GreatPosterWall.py` | `greatposterwall.com`| `GreatPosterWall.txt` | - |
| `HDCiTY.py` | `hdcity.city`| `HDCiTY.txt` | - |
| `HDSpace.py` | `hd-space.org`| `HDSpace.txt` | - |
| `HDTorrents.py` | `hd-torrents.org`| `HDTorrents.txt` | ⚠️ PTN spoofs the User-Agent for it to work. |
| `ImmortalSeed.py` | `immortalseed.me`| `ImmortalSeed.txt` | - |
| `IPTorrents.py` | `iptorrents.com`| `IPTorrents.txt` | - |
| `Lajidui.py` | `pt.lajidui.top`| `Lajidui.txt` | - |
| `March.py` | `duckboobee.org`| `March.txt` | - |
| `Orpheus.py` | `orpheus.network`| `Orpheus.txt` | It is necessary to add an API key in config.py |
| `PTFans.py` | `ptfans.cc`| `PTFans.txt` | No longer maintained, support is provided as-is. |
| `PTSKit.py` | `ptskit.org`| `PTSKit.txt` | - |
| `SceneTime.py` | `scenetime.com`| `SceneTime.txt` | - |
| `SportsCult.py` | `sportscult.org`| `SportsCult.txt` | - |
| `TorrentDay.py` | `torrentday.com`| `TorrentDay.txt` | - |
| `TorrentLeech.py` | `torrentleech.org`| `TorrentLeech.txt` | Currently only supports site notifications, not private messages. |

## Setup

### Telegram Bot Setup

<details>
<summary>Click to reveal instructions for creating a bot and obtaining your Chat ID</summary>

#### Step 1: Create your Telegram Bot

The bot acts as the delivery agent for your notifications.

1. In Telegram, search for **@BotFather**.
2. Send the command: `/newbot`
3. Provide a display name for the bot (e.g., PT_Notifier).
4. Create a unique username ending in "bot" (e.g., MyPrivateNotifier_bot).
5. BotFather will provide an API TOKEN. Save this string; it is your **TELEGRAM_BOT_TOKEN**.

#### Step 2: Retrieve your Chat ID

You can receive notifications in a private chat or a group.

1. Create a group and add your bot to it, or start a private conversation with the bot.
2. To find the unique ID of that chat, search for and use **@userinfobot** ([https://t.me/userinfobot](https://t.me/userinfobot)).
3. Send a message to that bot or forward a message from your group to it.
4. The bot will return a numerical ID. Group IDs typically begin with a minus sign (e.g., -100123456789). This is your **TELEGRAM_CHAT_ID**.

</details>

### Discord Webhook Setup

<details>
<summary>Click to reveal instructions for creating a Discord Webhook</summary>

1. Open the Discord channel where you want to receive notifications.
2. From the channel menu, select **Edit channel**.
3. Select **Integrations**.
4. Select **Webhooks**.
5. Click **New Webhook**.
6. Set a name for the webhook (e.g., PTNotifier).
7. Click **Copy Webhook URL**. This is your **`DISCORD_WEBHOOK_URL`**.

</details>

Follow these steps to set up PTNotifier.

### 1. Prerequisites

-   Python 3.10 or higher.
-   A Telegram Bot Token and a Chat ID, and/or a Discord Webhook URL.

### 2. Clone the Repository

Clone this repository to your local machine:

```bash
git clone https://github.com/wastaken7/PTNotifier.git
cd PTNotifier
```

### 3. Install Dependencies

Install the required Python packages using pip:

```bash
pip install -r requirements.txt
```

### 4. Configure the Application

Run the script:
```bash
python ptn.py
```

The first time you run the script, it will create a `config.py` file from `example-config.py`. You must edit this file with your settings.

-   **`TELEGRAM_BOT_TOKEN`**: Your Telegram bot's API token.
-   **`TELEGRAM_CHAT_ID`**: The ID of the Telegram chat where you want to receive notifications. You can also provide a `TELEGRAM_TOPIC_ID` if you want to send messages to a specific topic in a group.
-   **`DISCORD_WEBHOOK_URL`**: Your Discord webhook URL.
-   **`CHECK_INTERVAL`**: The time in seconds between checks. The minimum is 900 seconds (15 minutes) to avoid spamming trackers. Please note that some trackers have specific rules regarding the frequency of automated requests, and PTN will automatically adjust the interval if it is set too low for that specific tracker.
-   **`MARK_AS_READ`**: (Optional) For some trackers, the script can attempt to mark notifications as read. Set to `True` or `False`.
-   **`TIMEOUT`**: The timeout in seconds for network requests.
-   **`REQUEST_DELAY`**: Delay in seconds between requests to avoid being rate-limited.

### 5. Add Tracker Cookies

This tool requires cookies to access your tracker accounts. You must export them from your browser in the **Netscape** format. A recommended browser extension for this is [Get cookies.txt LOCALLY](https://chromewebstore.google.com/detail/get-cookiestxt-locally/cclelndahbckbenkjhflpdbgdldlbecc) (Chromium Browsers) or [cookies.txt](https://addons.mozilla.org/en-US/firefox/addon/cookies-txt/) (Firefox) or a similar one.

**Please note that the exported cookie file is linked to your session. If you log out of the website, PTN will no longer work for that website and you will need to export it again.**

1.  Log into your tracker account in your browser.
2.  Use your chosen extension to export the cookies **for that tracker's** domain as a `.txt` file.
3.  Save the exported cookie file into the correct subdirectory based on the tracker type, following the naming rules in the table above.

-   For **AvistaZ** or **UNIT3D** trackers, save the file in `cookies/AvistaZ/` or `cookies/UNIT3D/`. The filename can be anything (e.g., `my_cookie.txt`).
-   For trackers listed in the **Other** category, you must save the file in `cookies/Other/` and use the specific filename from the table above (e.g., `GreatPosterWall.txt` for GreatPosterWall).

The final directory structure should look like this:
```
c:\PTNotifier\
├───cookies\
│   ├───AvistaZ\
│   │   └───avistaz_user.txt
│   ├───Other\
│   │   ├───GreatPosterWall.txt
│   │   └───Anthelion.txt
│   └───UNIT3D\
│       └───my_unit3d_site.txt
└───...
```

## Usage

Once everything is configured, you can run the notifier:

```bash
python ptn.py
```

The script will start, load all trackers with valid cookie files, and begin monitoring. The first run for each tracker will not send any notifications; it will only establish a baseline of existing items.

## Disclaimer

Using scripts to interact with tracker sites may be against their rules. Use this tool at your own risk. The developer is not responsible for any consequences that may arise from its use. Always respect the tracker's rules and set a reasonable `CHECK_INTERVAL` to avoid getting your account banned.

