# Anime Wordle

A Wordle-inspired guessing game for anime characters.
Try to guess in the minimum number of attempts!

Play directly [here](https://hewigkeyt.github.io/anime_wordle/)!

## Current status

The game has a leaderboard for the daily guess. It is split between no-hint/with-hint players.
You can also see the ranking from the previous day.

The feedback you get is based on Anime, anime year, studio, height, age, hair color, seiyuu (Japanese voice actor) and sex.
Studio and hair color may have multiple values.
Age, height and sex may have unknown value.

> ⚠️ **Note on Character Pool:** 
> The character pool is currently limited and may contain occasional inaccuracies or formatting errors in character attributes. 

If you notice any missing characters, incorrect data, or bugs, please feel free to **open an issue/ticket** in this repository. Community contributions and feedback are highly appreciated to help expand and refine the database!

## Installation & Setup
If you need it locally

```bash
# Clone the repository
git clone https://github.com/Hewigkeyt/anime_wordle.git

# Navigate into the directory
cd anime_wordle

# Install modules
npm install

# Start
npm run dev
```
You would need a .env file with your own VITE_SUPABASE_URL and VITE_SUPABASE_ANON_KEY.

An optimal solver bot lives in [solver/](solver/README.md).


### Important changelog
- 2026-07-29 Big DB update. Brown hair split into brown, dark brown and light brown.
- 2026-07-22 Added a monthly ranking! The average is calculated with the following weights.\
Hints used = +5. Missed day = worst score of the day +1.
- 2026-07-19 Removed by default suggestions from animes already ruled out.                     
- 2026-07-19 Revamped the hint panel to filter directly the ruled out studios, and to show studios' year range.
- 2026-07-14 Updated in the daily picker so the same character cannot be picked twice in 3 months.
- 2026-07-10 Added multi-value for hair color (removed "bicolor" value) and studio.
- 2026-07-09 Changed the leaderboard layout and added results for previous daily.
- 2026-07-02 Added seiyuu and sex data in the clues.