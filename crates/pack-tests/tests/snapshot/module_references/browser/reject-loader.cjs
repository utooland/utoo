module.exports = () => {
  throw new Error("A user loader must not read the replaced implementation");
};
